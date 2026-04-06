mod buddy;
mod capabilities;
mod cockpit;
mod config;
mod executor;
mod fleet;
mod memory;
mod mission;
mod paths;
mod state;
mod tui;

use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::{thread, time::Duration};

use serde::Serialize;
use serde_json::{Value, json};

use buddy::buddy_ask;
use capabilities::{AGENTS, MINIMAL_STACK, SKILLS, VERSION, WORKFLOW_STACK};
use cockpit::{
    capture_pane, cockpit_attach, cockpit_doctor, cockpit_open, focus_machine, restart_pane,
    runtime_status, send_to_pane,
};
use config::{fleet_machine, load_fleet_config, load_memory_config, load_models_config};
use executor::{run_mission, summarize_mission_command, verify_mission};
use fleet::{bridge_sync, fleet_check};
use memory::summarize_mission_to_memory;
use mission::{delegate_step, plan_mission, retry_mission};
use state::{
    append_event, list_missions, load_mission, mission_events_text, mission_summary_value,
    new_mission, save_mission,
};
use tui::{TuiAction, run_tui};

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<ExitCode, String> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        return default_behavior();
    }

    match args[0].as_str() {
        "-V" | "--version" => {
            println!("{VERSION}");
            Ok(ExitCode::SUCCESS)
        }
        "-h" | "--help" | "help" => {
            print_help();
            Ok(ExitCode::SUCCESS)
        }
        "skills" => handle_skills(&args[1..]),
        "agents" => handle_agents(&args[1..]),
        "doctor" => handle_doctor(&args[1..]),
        "tui" => handle_tui(&args[1..]),
        "cockpit" => handle_cockpit(&args[1..]),
        "mission" => handle_mission(&args[1..]),
        "delegate" => handle_delegate(&args[1..]),
        "buddy" => handle_buddy(&args[1..]),
        "memory" => handoff_to_python(&args),
        "fleet" => handle_fleet(&args[1..]),
        _ => handoff_to_python(&args),
    }
}

fn default_behavior() -> Result<ExitCode, String> {
    if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
        let cwd =
            env::current_dir().map_err(|err| format!("cannot resolve current directory: {err}"))?;
        let code = cockpit_open(
            &cwd.to_string_lossy(),
            "constant-fleet",
            "constant",
            false,
            false,
        )?;
        Ok(exit_code_from_status(Some(code)))
    } else {
        print_doctor(false)?;
        Ok(ExitCode::SUCCESS)
    }
}

fn print_help() {
    let prog = program_name();
    println!(
        "{prog} {VERSION}

Rust front-controller for Constant.

Commands handled in Rust:
  doctor [--json]
  agents [--json]
  skills [--json] [--public-only]
  tui [--workspace DIR] [--local-session NAME] [--session NAME]
  cockpit open|attach|status|doctor|focus|send|capture|restart
  mission create <prompt> [--workspace DIR] [--json]
  mission plan <mission_id> [--json]
  mission run <mission_id> [--json]
  mission status [mission_id] [--verbose] [--json]
  mission tail <mission_id> [--follow]
  mission verify <mission_id> [--step-id ID] [--json]
  mission retry <mission_id> [--step-id ID] [--json]
  mission summarize <mission_id> [--json]
  delegate <mission_id> [--step-id ID] [--machine LABEL] [--backend NAME] [--cli NAME] [--agent ID] [--skill ID] [--json]

Other commands still hand off to the existing Python runtime during migration.
Running `{prog}` with no arguments opens or attaches the full fleet cockpit.
Use `{prog} tui --workspace DIR` for the standalone TUI."
    );
}

fn program_name() -> String {
    env::var("CONSTANT_PROG_NAME").unwrap_or_else(|_| "constant".to_string())
}

fn handle_skills(args: &[String]) -> Result<ExitCode, String> {
    let mut as_json = false;
    let mut public_only = false;
    for arg in args {
        match arg.as_str() {
            "--json" => as_json = true,
            "--public-only" => public_only = true,
            _ => return handoff_to_python_with_prefix("skills", args),
        }
    }
    print_skills(as_json, public_only)?;
    Ok(ExitCode::SUCCESS)
}

fn handle_agents(args: &[String]) -> Result<ExitCode, String> {
    let mut as_json = false;
    for arg in args {
        match arg.as_str() {
            "--json" => as_json = true,
            _ => return handoff_to_python_with_prefix("agents", args),
        }
    }
    print_agents(as_json)?;
    Ok(ExitCode::SUCCESS)
}

fn handle_doctor(args: &[String]) -> Result<ExitCode, String> {
    let mut as_json = false;
    for arg in args {
        match arg.as_str() {
            "--json" => as_json = true,
            _ => return handoff_to_python_with_prefix("doctor", args),
        }
    }
    print_doctor(as_json)?;
    Ok(ExitCode::SUCCESS)
}

fn handle_tui(args: &[String]) -> Result<ExitCode, String> {
    let mut workspace = env::current_dir()
        .map_err(|err| format!("cannot resolve current directory: {err}"))?
        .to_string_lossy()
        .into_owned();
    let mut local_session = "constant-fleet".to_string();
    let mut machine_session = "constant".to_string();

    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--workspace" => {
                index += 1;
                workspace = args
                    .get(index)
                    .cloned()
                    .ok_or_else(|| "--workspace requires a value".to_string())?;
            }
            "--local-session" => {
                index += 1;
                local_session = args
                    .get(index)
                    .cloned()
                    .ok_or_else(|| "--local-session requires a value".to_string())?;
            }
            "--session" => {
                index += 1;
                machine_session = args
                    .get(index)
                    .cloned()
                    .ok_or_else(|| "--session requires a value".to_string())?;
            }
            _ => return handoff_to_python_with_prefix("tui", args),
        }
        index += 1;
    }

    let workspace = canonical_workspace(&workspace)?;
    match run_tui(
        workspace.clone(),
        local_session.clone(),
        machine_session.clone(),
    )? {
        TuiAction::Exit => Ok(ExitCode::SUCCESS),
        TuiAction::OpenCockpit => Ok(exit_code_from_status(Some(cockpit_open(
            &workspace,
            &local_session,
            &machine_session,
            false,
            false,
        )?))),
    }
}

fn handle_cockpit(args: &[String]) -> Result<ExitCode, String> {
    if args.is_empty() {
        return Err("cockpit requires a subcommand".to_string());
    }
    match args[0].as_str() {
        "open" => cockpit_open_cmd(&args[1..]),
        "attach" => cockpit_attach_cmd(&args[1..]),
        "status" => cockpit_status_cmd(&args[1..]),
        "doctor" => cockpit_doctor_cmd(&args[1..]),
        "focus" => cockpit_focus_cmd(&args[1..]),
        "send" => cockpit_send_cmd(&args[1..]),
        "capture" => cockpit_capture_cmd(&args[1..]),
        "restart" => cockpit_restart_cmd(&args[1..]),
        _ => {
            let mut forwarded = vec!["cockpit".to_string()];
            forwarded.extend(args.iter().cloned());
            handoff_to_python(&forwarded)
        }
    }
}

fn handle_mission(args: &[String]) -> Result<ExitCode, String> {
    if args.is_empty() {
        return Err("mission requires a subcommand".to_string());
    }

    match args[0].as_str() {
        "create" => mission_create(&args[1..]),
        "plan" => mission_plan(&args[1..]),
        "run" => mission_run_cmd(&args[1..]),
        "status" => mission_status(&args[1..]),
        "tail" => mission_tail(&args[1..]),
        "verify" => mission_verify_cmd(&args[1..]),
        "retry" => mission_retry_cmd(&args[1..]),
        "summarize" => mission_summarize_cmd(&args[1..]),
        _ => {
            let mut forwarded = vec!["mission".to_string()];
            forwarded.extend(args.iter().cloned());
            handoff_to_python(&forwarded)
        }
    }
}

fn handle_buddy(args: &[String]) -> Result<ExitCode, String> {
    if args.is_empty() {
        return Err("buddy requires a subcommand".to_string());
    }
    match args[0].as_str() {
        "ask" => buddy_ask_cmd(&args[1..]),
        _ => {
            let mut forwarded = vec!["buddy".to_string()];
            forwarded.extend(args.iter().cloned());
            handoff_to_python(&forwarded)
        }
    }
}

fn handle_fleet(args: &[String]) -> Result<ExitCode, String> {
    if args.is_empty() {
        return Err("fleet requires a subcommand".to_string());
    }
    match args[0].as_str() {
        "status" => fleet_status_cmd(&args[1..]),
        "sync" => fleet_sync_cmd(&args[1..]),
        _ => {
            let mut forwarded = vec!["fleet".to_string()];
            forwarded.extend(args.iter().cloned());
            handoff_to_python(&forwarded)
        }
    }
}

fn mission_create(args: &[String]) -> Result<ExitCode, String> {
    let mut prompt: Option<String> = None;
    let mut workspace = env::current_dir()
        .map_err(|err| format!("cannot resolve current directory: {err}"))?
        .to_string_lossy()
        .into_owned();
    let mut as_json = false;

    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--workspace" => {
                index += 1;
                workspace = args
                    .get(index)
                    .cloned()
                    .ok_or_else(|| "--workspace requires a value".to_string())?;
            }
            "--json" => as_json = true,
            value if value.starts_with("--") => {
                return handoff_mission_subcommand("create", args);
            }
            value => {
                if prompt.is_some() {
                    return handoff_mission_subcommand("create", args);
                }
                prompt = Some(value.to_string());
            }
        }
        index += 1;
    }

    let prompt = prompt.ok_or_else(|| "mission create requires a prompt".to_string())?;
    let models = load_models_config()?;
    let fleet = load_fleet_config()?;
    let workspace = canonical_workspace(&workspace)?;

    let mut mission = new_mission(&prompt, &workspace, None, &models);
    save_mission(&mut mission)?;
    append_event(
        &mission.mission_id,
        "mission.created",
        json!({ "goal": prompt, "workspace": workspace }),
    )?;
    let buddy_review = plan_mission(&mut mission, &fleet)?;
    save_mission(&mut mission)?;
    append_event(
        &mission.mission_id,
        "mission.planned",
        json!({
            "plan": {
                "title": mission.title,
                "summary": mission.planner_summary,
                "steps": mission.steps,
            },
            "buddy_review": buddy_review,
        }),
    )?;
    print_value(&mission_summary_value(&mission), as_json)?;
    Ok(ExitCode::SUCCESS)
}

fn mission_plan(args: &[String]) -> Result<ExitCode, String> {
    let mut mission_id: Option<String> = None;
    let mut as_json = false;
    for arg in args {
        match arg.as_str() {
            "--json" => as_json = true,
            value if value.starts_with("--") => return handoff_mission_subcommand("plan", args),
            value => {
                if mission_id.is_some() {
                    return handoff_mission_subcommand("plan", args);
                }
                mission_id = Some(value.to_string());
            }
        }
    }
    let mission_id = mission_id.ok_or_else(|| "mission plan requires a mission_id".to_string())?;
    let fleet = load_fleet_config()?;
    let mut mission = load_mission(&mission_id)?;
    let buddy_review = plan_mission(&mut mission, &fleet)?;
    save_mission(&mut mission)?;
    append_event(
        &mission.mission_id,
        "mission.replanned",
        json!({
            "plan": {
                "title": mission.title,
                "summary": mission.planner_summary,
                "steps": mission.steps,
            },
            "buddy_review": buddy_review,
        }),
    )?;
    print_value(
        &json!({
            "summary": mission_summary_value(&mission),
            "buddy_review": mission.buddy_review,
        }),
        as_json,
    )?;
    Ok(ExitCode::SUCCESS)
}

fn mission_run_cmd(args: &[String]) -> Result<ExitCode, String> {
    let mut mission_id: Option<String> = None;
    let mut as_json = false;
    for arg in args {
        match arg.as_str() {
            "--json" => as_json = true,
            value if value.starts_with("--") => return handoff_mission_subcommand("run", args),
            value => {
                if mission_id.is_some() {
                    return handoff_mission_subcommand("run", args);
                }
                mission_id = Some(value.to_string());
            }
        }
    }
    let mission_id = mission_id.ok_or_else(|| "mission run requires a mission_id".to_string())?;
    let fleet = load_fleet_config()?;
    let mut mission = load_mission(&mission_id)?;
    let (payload, ok) = run_mission(&mut mission, &fleet)?;
    print_value(&payload, as_json)?;
    Ok(exit_code_from_status(Some(if ok { 0 } else { 1 })))
}

fn mission_status(args: &[String]) -> Result<ExitCode, String> {
    let mut mission_id: Option<String> = None;
    let mut verbose = false;
    let mut as_json = false;
    for arg in args {
        match arg.as_str() {
            "--verbose" => verbose = true,
            "--json" => as_json = true,
            value if value.starts_with("--") => return handoff_mission_subcommand("status", args),
            value => {
                if mission_id.is_some() {
                    return handoff_mission_subcommand("status", args);
                }
                mission_id = Some(value.to_string());
            }
        }
    }

    if let Some(mission_id) = mission_id {
        let mission = load_mission(&mission_id)?;
        if verbose {
            print_value(
                &serde_json::to_value(&mission).map_err(|err| err.to_string())?,
                as_json,
            )?;
        } else {
            print_value(&mission_summary_value(&mission), as_json)?;
        }
        return Ok(ExitCode::SUCCESS);
    }

    let summaries = list_missions()?
        .into_iter()
        .map(|mission| mission_summary_value(&mission))
        .collect::<Vec<_>>();
    print_value(&json!({ "missions": summaries }), as_json)?;
    Ok(ExitCode::SUCCESS)
}

fn mission_tail(args: &[String]) -> Result<ExitCode, String> {
    let mut mission_id: Option<String> = None;
    let mut follow = false;
    for arg in args {
        match arg.as_str() {
            "--follow" => follow = true,
            value if value.starts_with("--") => return handoff_mission_subcommand("tail", args),
            value => {
                if mission_id.is_some() {
                    return handoff_mission_subcommand("tail", args);
                }
                mission_id = Some(value.to_string());
            }
        }
    }

    let mission_id = mission_id.ok_or_else(|| "mission tail requires a mission_id".to_string())?;
    if !follow {
        println!("{}", mission_events_text(&mission_id)?);
        return Ok(ExitCode::SUCCESS);
    }

    let path = paths::mission_events_file(&mission_id);
    if !path.exists() {
        return Err(format!("No events for mission {mission_id}"));
    }

    let mut seen = 0_usize;
    loop {
        let text = mission_events_text(&mission_id)?;
        if text.len() > seen {
            print!("{}", &text[seen..]);
            seen = text.len();
        }
        thread::sleep(Duration::from_secs(1));
    }
}

fn mission_verify_cmd(args: &[String]) -> Result<ExitCode, String> {
    let mut mission_id: Option<String> = None;
    let mut step_id: Option<String> = None;
    let mut as_json = false;

    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--step-id" => {
                index += 1;
                step_id = Some(
                    args.get(index)
                        .cloned()
                        .ok_or_else(|| "--step-id requires a value".to_string())?,
                );
            }
            "--json" => as_json = true,
            value if value.starts_with("--") => return handoff_mission_subcommand("verify", args),
            value => {
                if mission_id.is_some() {
                    return handoff_mission_subcommand("verify", args);
                }
                mission_id = Some(value.to_string());
            }
        }
        index += 1;
    }

    let mission_id =
        mission_id.ok_or_else(|| "mission verify requires a mission_id".to_string())?;
    let mut mission = load_mission(&mission_id)?;
    let (payload, ok) = verify_mission(&mut mission, step_id.as_deref())?;
    print_value(&payload, as_json)?;
    Ok(exit_code_from_status(Some(if ok { 0 } else { 1 })))
}

fn mission_retry_cmd(args: &[String]) -> Result<ExitCode, String> {
    let mut mission_id: Option<String> = None;
    let mut step_id: Option<String> = None;
    let mut as_json = false;

    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--step-id" => {
                index += 1;
                step_id = Some(
                    args.get(index)
                        .cloned()
                        .ok_or_else(|| "--step-id requires a value".to_string())?,
                );
            }
            "--json" => as_json = true,
            value if value.starts_with("--") => return handoff_mission_subcommand("retry", args),
            value => {
                if mission_id.is_some() {
                    return handoff_mission_subcommand("retry", args);
                }
                mission_id = Some(value.to_string());
            }
        }
        index += 1;
    }

    let mission_id = mission_id.ok_or_else(|| "mission retry requires a mission_id".to_string())?;
    let mut mission = load_mission(&mission_id)?;
    let payload = retry_mission(&mut mission, step_id.as_deref())?;
    save_mission(&mut mission)?;
    append_event(&mission_id, "step.retry_requested", payload)?;
    print_value(&mission_summary_value(&mission), as_json)?;
    Ok(ExitCode::SUCCESS)
}

fn mission_summarize_cmd(args: &[String]) -> Result<ExitCode, String> {
    let mut mission_id: Option<String> = None;
    let mut as_json = false;
    for arg in args {
        match arg.as_str() {
            "--json" => as_json = true,
            value if value.starts_with("--") => {
                return handoff_mission_subcommand("summarize", args);
            }
            value => {
                if mission_id.is_some() {
                    return handoff_mission_subcommand("summarize", args);
                }
                mission_id = Some(value.to_string());
            }
        }
    }
    let mission_id =
        mission_id.ok_or_else(|| "mission summarize requires a mission_id".to_string())?;
    let payload = summarize_mission_command(&mission_id)?;
    let mission = load_mission(&mission_id)?;
    let memory = summarize_mission_to_memory(&mission)?;
    print_value(
        &json!({ "artifact_summary": payload, "memory_summary": memory }),
        as_json,
    )?;
    Ok(ExitCode::SUCCESS)
}

fn handle_delegate(args: &[String]) -> Result<ExitCode, String> {
    if args.is_empty() {
        return Err("delegate requires a mission_id".to_string());
    }
    let mission_id = args[0].clone();
    let mut step_id: Option<String> = None;
    let mut machine: Option<String> = None;
    let mut backend: Option<String> = None;
    let mut cli: Option<String> = None;
    let mut agent: Option<String> = None;
    let mut skill: Option<String> = None;
    let mut as_json = false;

    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--step-id" => {
                index += 1;
                step_id = Some(
                    args.get(index)
                        .cloned()
                        .ok_or_else(|| "--step-id requires a value".to_string())?,
                );
            }
            "--machine" => {
                index += 1;
                machine = Some(
                    args.get(index)
                        .cloned()
                        .ok_or_else(|| "--machine requires a value".to_string())?,
                );
            }
            "--backend" => {
                index += 1;
                backend = Some(
                    args.get(index)
                        .cloned()
                        .ok_or_else(|| "--backend requires a value".to_string())?,
                );
            }
            "--cli" => {
                index += 1;
                cli = Some(
                    args.get(index)
                        .cloned()
                        .ok_or_else(|| "--cli requires a value".to_string())?,
                );
            }
            "--agent" => {
                index += 1;
                agent = Some(
                    args.get(index)
                        .cloned()
                        .ok_or_else(|| "--agent requires a value".to_string())?,
                );
            }
            "--skill" => {
                index += 1;
                skill = Some(
                    args.get(index)
                        .cloned()
                        .ok_or_else(|| "--skill requires a value".to_string())?,
                );
            }
            "--json" => as_json = true,
            value if value.starts_with("--") => {
                return handoff_to_python_with_prefix("delegate", args);
            }
            _ => return handoff_to_python_with_prefix("delegate", args),
        }
        index += 1;
    }

    let mut mission = load_mission(&mission_id)?;
    if let Some(machine) = machine.as_deref() {
        let fleet = load_fleet_config()?;
        fleet_machine(&fleet, machine)?;
    }
    let payload = delegate_step(
        &mut mission,
        step_id.as_deref(),
        machine.as_deref(),
        backend.as_deref(),
        cli.as_deref(),
        agent.as_deref(),
        skill.as_deref(),
    )?;
    save_mission(&mut mission)?;
    append_event(&mission_id, "step.delegated", payload)?;
    print_value(&mission_summary_value(&mission), as_json)?;
    Ok(ExitCode::SUCCESS)
}

fn buddy_ask_cmd(args: &[String]) -> Result<ExitCode, String> {
    let mut mission_id: Option<String> = None;
    let mut prompt: Option<String> = None;
    let mut as_json = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--mission-id" => {
                index += 1;
                mission_id = Some(
                    args.get(index)
                        .cloned()
                        .ok_or_else(|| "--mission-id requires a value".to_string())?,
                );
            }
            "--json" => as_json = true,
            value if value.starts_with("--") => {
                let mut forwarded = vec!["buddy".to_string(), "ask".to_string()];
                forwarded.extend(args.iter().cloned());
                return handoff_to_python(&forwarded);
            }
            value => {
                if prompt.is_some() {
                    let mut forwarded = vec!["buddy".to_string(), "ask".to_string()];
                    forwarded.extend(args.iter().cloned());
                    return handoff_to_python(&forwarded);
                }
                prompt = Some(value.to_string());
            }
        }
        index += 1;
    }
    let prompt = prompt.ok_or_else(|| "buddy ask requires a prompt".to_string())?;
    let mission = mission_id.as_deref().map(load_mission).transpose()?;
    let workspace = mission.as_ref().map(|entry| entry.workspace.as_str());
    let payload = buddy_ask(mission.as_ref(), &prompt, workspace)?;
    print_value(&payload, as_json)?;
    Ok(ExitCode::SUCCESS)
}

fn fleet_status_cmd(args: &[String]) -> Result<ExitCode, String> {
    let as_json = args.iter().any(|arg| arg == "--json");
    let payload = fleet_check()?;
    let ok = payload
        .get("returncode")
        .and_then(Value::as_i64)
        .unwrap_or(1)
        == 0;
    print_value(&payload, as_json)?;
    Ok(exit_code_from_status(Some(if ok { 0 } else { 1 })))
}

fn fleet_sync_cmd(args: &[String]) -> Result<ExitCode, String> {
    let as_json = args.iter().any(|arg| arg == "--json");
    let payload = bridge_sync()?;
    let ok = payload
        .get("returncode")
        .and_then(Value::as_i64)
        .unwrap_or(1)
        == 0;
    print_value(&payload, as_json)?;
    Ok(exit_code_from_status(Some(if ok { 0 } else { 1 })))
}

fn handoff_mission_subcommand(prefix: &str, tail: &[String]) -> Result<ExitCode, String> {
    let mut args = vec!["mission".to_string(), prefix.to_string()];
    args.extend(tail.iter().cloned());
    handoff_to_python(&args)
}

fn handoff_cockpit_subcommand(prefix: &str, tail: &[String]) -> Result<ExitCode, String> {
    let mut args = vec!["cockpit".to_string(), prefix.to_string()];
    args.extend(tail.iter().cloned());
    handoff_to_python(&args)
}

fn cockpit_open_cmd(args: &[String]) -> Result<ExitCode, String> {
    let mut workspace = env::current_dir()
        .map_err(|err| format!("cannot resolve current directory: {err}"))?
        .to_string_lossy()
        .into_owned();
    let mut local_session = "constant-fleet".to_string();
    let mut machine_session = "constant".to_string();
    let mut recreate = false;
    let mut remote_recreate = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--workspace" => {
                index += 1;
                workspace = args
                    .get(index)
                    .cloned()
                    .ok_or_else(|| "--workspace requires a value".to_string())?;
            }
            "--local-session" => {
                index += 1;
                local_session = args
                    .get(index)
                    .cloned()
                    .ok_or_else(|| "--local-session requires a value".to_string())?;
            }
            "--session" => {
                index += 1;
                machine_session = args
                    .get(index)
                    .cloned()
                    .ok_or_else(|| "--session requires a value".to_string())?;
            }
            "--recreate" => recreate = true,
            "--remote-recreate" => remote_recreate = true,
            _ => return handoff_cockpit_subcommand("open", args),
        }
        index += 1;
    }
    let code = cockpit_open(
        &workspace,
        &local_session,
        &machine_session,
        recreate,
        remote_recreate,
    )?;
    Ok(exit_code_from_status(Some(code)))
}

fn cockpit_attach_cmd(args: &[String]) -> Result<ExitCode, String> {
    let mut local_session = "constant-fleet".to_string();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--local-session" => {
                index += 1;
                local_session = args
                    .get(index)
                    .cloned()
                    .ok_or_else(|| "--local-session requires a value".to_string())?;
            }
            _ => return handoff_cockpit_subcommand("attach", args),
        }
        index += 1;
    }
    let code = cockpit_attach(&local_session)?;
    Ok(exit_code_from_status(Some(code)))
}

fn cockpit_status_cmd(args: &[String]) -> Result<ExitCode, String> {
    let mut local_session = "constant-fleet".to_string();
    let mut machine_session = "constant".to_string();
    let mut as_json = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--local-session" => {
                index += 1;
                local_session = args
                    .get(index)
                    .cloned()
                    .ok_or_else(|| "--local-session requires a value".to_string())?;
            }
            "--session" => {
                index += 1;
                machine_session = args
                    .get(index)
                    .cloned()
                    .ok_or_else(|| "--session requires a value".to_string())?;
            }
            "--json" => as_json = true,
            _ => return handoff_cockpit_subcommand("status", args),
        }
        index += 1;
    }
    let payload = runtime_status(&local_session, &machine_session)?;
    print_value(&payload, as_json)?;
    Ok(ExitCode::SUCCESS)
}

fn cockpit_doctor_cmd(args: &[String]) -> Result<ExitCode, String> {
    let mut local_session = "constant-fleet".to_string();
    let mut machine_session = "constant".to_string();
    let mut as_json = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--local-session" => {
                index += 1;
                local_session = args
                    .get(index)
                    .cloned()
                    .ok_or_else(|| "--local-session requires a value".to_string())?;
            }
            "--session" => {
                index += 1;
                machine_session = args
                    .get(index)
                    .cloned()
                    .ok_or_else(|| "--session requires a value".to_string())?;
            }
            "--json" => as_json = true,
            _ => return handoff_cockpit_subcommand("doctor", args),
        }
        index += 1;
    }
    let payload = cockpit_doctor(&local_session, &machine_session)?;
    print_value(&payload, as_json)?;
    Ok(ExitCode::SUCCESS)
}

fn cockpit_focus_cmd(args: &[String]) -> Result<ExitCode, String> {
    let mut machine: Option<String> = None;
    let mut pane: Option<String> = None;
    let mut local_session = "constant-fleet".to_string();
    let mut machine_session = "constant".to_string();
    let mut as_json = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--machine" => {
                index += 1;
                machine = Some(
                    args.get(index)
                        .cloned()
                        .ok_or_else(|| "--machine requires a value".to_string())?,
                );
            }
            "--pane" => {
                index += 1;
                pane = Some(
                    args.get(index)
                        .cloned()
                        .ok_or_else(|| "--pane requires a value".to_string())?,
                );
            }
            "--local-session" => {
                index += 1;
                local_session = args
                    .get(index)
                    .cloned()
                    .ok_or_else(|| "--local-session requires a value".to_string())?;
            }
            "--session" => {
                index += 1;
                machine_session = args
                    .get(index)
                    .cloned()
                    .ok_or_else(|| "--session requires a value".to_string())?;
            }
            "--json" => as_json = true,
            _ => return handoff_cockpit_subcommand("focus", args),
        }
        index += 1;
    }
    let machine = machine.ok_or_else(|| "--machine is required".to_string())?;
    let payload = focus_machine(&machine, pane.as_deref(), &local_session, &machine_session)?;
    print_value(&payload, as_json)?;
    Ok(exit_code_from_status(Some(payload.returncode)))
}

fn cockpit_send_cmd(args: &[String]) -> Result<ExitCode, String> {
    let mut machine: Option<String> = None;
    let mut pane: Option<String> = None;
    let mut command: Option<String> = None;
    let mut machine_session = "constant".to_string();
    let mut as_json = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--machine" => {
                index += 1;
                machine = Some(
                    args.get(index)
                        .cloned()
                        .ok_or_else(|| "--machine requires a value".to_string())?,
                );
            }
            "--pane" => {
                index += 1;
                pane = Some(
                    args.get(index)
                        .cloned()
                        .ok_or_else(|| "--pane requires a value".to_string())?,
                );
            }
            "--command" => {
                index += 1;
                command = Some(
                    args.get(index)
                        .cloned()
                        .ok_or_else(|| "--command requires a value".to_string())?,
                );
            }
            "--session" => {
                index += 1;
                machine_session = args
                    .get(index)
                    .cloned()
                    .ok_or_else(|| "--session requires a value".to_string())?;
            }
            "--json" => as_json = true,
            _ => return handoff_cockpit_subcommand("send", args),
        }
        index += 1;
    }
    let payload = send_to_pane(
        &machine.ok_or_else(|| "--machine is required".to_string())?,
        &pane.ok_or_else(|| "--pane is required".to_string())?,
        &command.ok_or_else(|| "--command is required".to_string())?,
        &machine_session,
    )?;
    print_value(&payload, as_json)?;
    Ok(exit_code_from_status(Some(payload.returncode)))
}

fn cockpit_capture_cmd(args: &[String]) -> Result<ExitCode, String> {
    let mut machine: Option<String> = None;
    let mut pane: Option<String> = None;
    let mut lines: u32 = 120;
    let mut machine_session = "constant".to_string();
    let mut as_json = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--machine" => {
                index += 1;
                machine = Some(
                    args.get(index)
                        .cloned()
                        .ok_or_else(|| "--machine requires a value".to_string())?,
                );
            }
            "--pane" => {
                index += 1;
                pane = Some(
                    args.get(index)
                        .cloned()
                        .ok_or_else(|| "--pane requires a value".to_string())?,
                );
            }
            "--lines" => {
                index += 1;
                lines = args
                    .get(index)
                    .ok_or_else(|| "--lines requires a value".to_string())?
                    .parse()
                    .map_err(|_| "--lines must be an integer".to_string())?;
            }
            "--session" => {
                index += 1;
                machine_session = args
                    .get(index)
                    .cloned()
                    .ok_or_else(|| "--session requires a value".to_string())?;
            }
            "--json" => as_json = true,
            _ => return handoff_cockpit_subcommand("capture", args),
        }
        index += 1;
    }
    let payload = capture_pane(
        &machine.ok_or_else(|| "--machine is required".to_string())?,
        &pane.ok_or_else(|| "--pane is required".to_string())?,
        lines,
        &machine_session,
    )?;
    if as_json {
        print_value(&payload, true)?;
    } else {
        println!("{}", payload.stdout);
    }
    Ok(exit_code_from_status(Some(payload.returncode)))
}

fn cockpit_restart_cmd(args: &[String]) -> Result<ExitCode, String> {
    let mut machine: Option<String> = None;
    let mut pane: Option<String> = None;
    let mut machine_session = "constant".to_string();
    let mut as_json = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--machine" => {
                index += 1;
                machine = Some(
                    args.get(index)
                        .cloned()
                        .ok_or_else(|| "--machine requires a value".to_string())?,
                );
            }
            "--pane" => {
                index += 1;
                pane = Some(
                    args.get(index)
                        .cloned()
                        .ok_or_else(|| "--pane requires a value".to_string())?,
                );
            }
            "--session" => {
                index += 1;
                machine_session = args
                    .get(index)
                    .cloned()
                    .ok_or_else(|| "--session requires a value".to_string())?;
            }
            "--json" => as_json = true,
            _ => return handoff_cockpit_subcommand("restart", args),
        }
        index += 1;
    }
    let payload = restart_pane(
        &machine.ok_or_else(|| "--machine is required".to_string())?,
        &pane.ok_or_else(|| "--pane is required".to_string())?,
        &machine_session,
    )?;
    print_value(&payload, as_json)?;
    Ok(exit_code_from_status(Some(payload.returncode)))
}

fn handoff_to_python_with_prefix(prefix: &str, tail: &[String]) -> Result<ExitCode, String> {
    let mut args = vec![prefix.to_string()];
    args.extend(tail.iter().cloned());
    handoff_to_python(&args)
}

fn handoff_to_python(args: &[String]) -> Result<ExitCode, String> {
    let repo_root = paths::repo_root();
    let default_python = paths::home_dir()
        .map(|home| home.join(".local/share/constant/venv/bin/python3"))
        .filter(|path| path.exists());
    let python_bin = env::var_os("CONSTANT_PYTHON")
        .or_else(|| default_python.map(Into::into))
        .unwrap_or_else(|| OsString::from("python3"));

    let mut command = Command::new(python_bin);
    command.arg("-m").arg("constant");
    command.args(args);
    command.env("CONSTANT_PROG_NAME", program_name());
    command.env("CONSTANT_RUST_HANDOFF", "1");
    command.env("PYTHONPATH", python_path_with_repo(&repo_root));
    let status = command
        .status()
        .map_err(|err| format!("python handoff failed: {err}"))?;

    Ok(exit_code_from_status(status.code()))
}

fn python_path_with_repo(repo_root: &Path) -> OsString {
    let mut value = OsString::from(repo_root.as_os_str());
    if let Some(existing) = env::var_os("PYTHONPATH") {
        value.push(":");
        value.push(existing);
    }
    value
}

fn canonical_workspace(input: &str) -> Result<String, String> {
    let path = PathBuf::from(input)
        .expand_home()
        .canonicalize()
        .or_else(|_| Ok(PathBuf::from(input).expand_home()))
        .map_err(|err: std::io::Error| format!("cannot resolve workspace {input}: {err}"))?;
    Ok(path.display().to_string())
}

fn print_agents(as_json: bool) -> Result<(), String> {
    let payload = json!({
        "agents": AGENTS,
        "recommended_skill_stack": recommended_skill_stack_value(),
    });
    if as_json {
        print_value(&payload, true)?;
    } else {
        println!("Agents");
        for agent in AGENTS {
            println!(
                "- {} ({}) :: cli={} layer={} manual_only={}",
                agent.id,
                agent.label,
                agent.primary_cli,
                agent.preferred_layers.join(","),
                agent.manual_only
            );
        }
    }
    Ok(())
}

fn print_skills(as_json: bool, public_only: bool) -> Result<(), String> {
    let skills = SKILLS
        .iter()
        .filter(|skill| !public_only || skill.visibility == "public")
        .collect::<Vec<_>>();

    let payload = json!({
        "skills": skills,
        "catalog": {
            "reflection": skills.iter().filter(|skill| skill.layer == "reflection").collect::<Vec<_>>(),
            "execution-prep": skills.iter().filter(|skill| skill.layer == "execution-prep").collect::<Vec<_>>(),
            "execution": skills.iter().filter(|skill| skill.layer == "execution").collect::<Vec<_>>(),
            "manual": [],
        },
        "recommended_skill_stack": recommended_skill_stack_value(),
    });

    if as_json {
        print_value(&payload, true)?;
    } else {
        println!("Skills");
        for skill in skills {
            println!(
                "- {} [{}] :: {} -> {}/{}",
                skill.id, skill.layer, skill.summary, skill.preferred_agent, skill.preferred_cli
            );
        }
    }
    Ok(())
}

fn print_doctor(as_json: bool) -> Result<(), String> {
    let payload = doctor_value()?;
    print_value(&payload, as_json)
}

fn doctor_value() -> Result<Value, String> {
    let fleet = load_fleet_config()?;
    let models = load_models_config()?;
    let memory = load_memory_config()?;

    Ok(json!({
        "version": VERSION,
        "repo_root": paths::repo_root().display().to_string(),
        "cache_root": paths::cache_root().display().to_string(),
        "config_root": paths::config_root().display().to_string(),
        "data_root": paths::data_root().display().to_string(),
        "rust": {
            "front_controller": true,
            "python_handoff": command_exists("python3"),
        },
        "commands": {
            "python3": command_exists("python3"),
            "tmux": command_exists("tmux"),
            "cargo": command_exists("cargo"),
            "claude": command_exists("claude"),
            "codex": command_exists("codex"),
            "copilot": command_exists("copilot"),
            "vibe": command_exists("vibe"),
        },
        "config": {
            "fleet": paths::fleet_toml_path().display().to_string(),
            "models": paths::models_toml_path().display().to_string(),
            "memory": paths::memory_toml_path().display().to_string(),
        },
        "fleet": {
            "local_machine": fleet.local_machine,
            "repo_dir": fleet.repo_dir,
            "machines": fleet.machines,
        },
        "models": models,
        "memory": memory,
        "wrapper": wrapper_status_value(),
    }))
}

fn wrapper_status_value() -> Value {
    let wrapper_dir = paths::cache_root().join("wrapper");
    let rust_bin = paths::repo_root().join("target/debug/constant");
    json!({
        "wrapper_dir": wrapper_dir.display().to_string(),
        "rust_bin": rust_bin.display().to_string(),
        "rust_bin_exists": rust_bin.exists(),
        "forced_python": env::var("CONSTANT_USE_PYTHON").ok().as_deref() == Some("1"),
        "forced_rust": env::var("CONSTANT_USE_RUST").ok().as_deref() == Some("1"),
        "force_recheck": env::var("CONSTANT_RUST_RECHECK").ok().as_deref() == Some("1"),
        "last_mode": read_wrapper_mode(&wrapper_dir.join("last-mode")),
        "cache": {
            "rust_ok": read_wrapper_stamp(&wrapper_dir.join("rust-ok")),
            "rust_fail": read_wrapper_stamp(&wrapper_dir.join("rust-fail")),
        },
        "codesign_verify": if rust_bin.exists() { Some(run_quick_command(&["codesign", "--verify", "--verbose=2", &rust_bin.display().to_string()])) } else { None },
        "spctl_assess": if rust_bin.exists() { Some(run_quick_command(&["spctl", "--assess", "-vv", &rust_bin.display().to_string()])) } else { None },
        "xattr_provenance": if rust_bin.exists() { Some(run_quick_command(&["xattr", "-p", "com.apple.provenance", &rust_bin.display().to_string()])) } else { None },
        "hint": "If spctl rejects the Rust binary from a normal terminal, allow that terminal in System Settings -> Privacy & Security -> Developer Tools, then rerun with CONSTANT_RUST_RECHECK=1 to force a fresh startup probe.",
    })
}

fn read_wrapper_stamp(path: &Path) -> Value {
    let Ok(raw) = fs::read_to_string(path) else {
        return Value::Null;
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Value::Null;
    }
    let parts: Vec<&str> = trimmed.split_whitespace().collect();
    if parts.len() < 2 {
        return json!({ "raw": trimmed });
    }
    let mut payload = json!({
        "signature": parts[0],
        "epoch": parts[1],
    });
    if let Ok(epoch) = parts[1].parse::<u64>() {
        if let Some(object) = payload.as_object_mut() {
            object.insert("timestamp".to_string(), json!(epoch));
        }
    }
    payload
}

fn read_wrapper_mode(path: &Path) -> Value {
    let Ok(raw) = fs::read_to_string(path) else {
        return Value::Null;
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Value::Null;
    }
    let parts: Vec<&str> = trimmed.split_whitespace().collect();
    if parts.len() < 3 {
        return json!({ "raw": trimmed });
    }
    json!({
        "mode": parts[0],
        "reason": parts[1],
        "epoch": parts[2],
    })
}

fn run_quick_command(command: &[&str]) -> Value {
    if command.is_empty() {
        return json!({ "ok": false, "error": "empty command" });
    }
    let mut cmd = Command::new(command[0]);
    if command.len() > 1 {
        cmd.args(&command[1..]);
    }
    match cmd.output() {
        Ok(output) => json!({
            "ok": output.status.success(),
            "returncode": output.status.code(),
            "stdout": String::from_utf8_lossy(&output.stdout).trim(),
            "stderr": String::from_utf8_lossy(&output.stderr).trim(),
            "command": command,
        }),
        Err(err) => json!({
            "ok": false,
            "error": err.to_string(),
            "command": command,
        }),
    }
}

fn recommended_skill_stack_value() -> Value {
    json!({
        "minimal": MINIMAL_STACK,
        "workflow": WORKFLOW_STACK,
        "layers": {
            "reflection": ["spec-planner", "architecture-brainstorm"],
            "execution": ["repo-onboarding", "task-decomposer", "pr-review-prep"],
        }
    })
}

fn print_value<T>(value: &T, as_json: bool) -> Result<(), String>
where
    T: Serialize,
{
    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(value)
                .map_err(|err| format!("cannot encode JSON: {err}"))?
        );
    } else {
        println!(
            "{}",
            serde_json::to_string_pretty(value)
                .map_err(|err| format!("cannot encode JSON: {err}"))?
        );
    }
    Ok(())
}

fn command_exists(binary: &str) -> bool {
    let Some(path) = env::var_os("PATH") else {
        return false;
    };
    env::split_paths(&path).any(|dir| dir.join(binary).exists())
}

fn exit_code_from_status(code: Option<i32>) -> ExitCode {
    match code {
        Some(value) if (0..=255).contains(&value) => ExitCode::from(value as u8),
        Some(_) => ExitCode::from(1),
        None => ExitCode::from(1),
    }
}

trait ExpandHomePath {
    fn expand_home(self) -> PathBuf;
}

impl ExpandHomePath for PathBuf {
    fn expand_home(self) -> PathBuf {
        let value = self.to_string_lossy().to_string();
        expand_home_string(&value)
    }
}

fn expand_home_string(value: &str) -> PathBuf {
    if value == "~" {
        return paths::home_dir().unwrap_or_else(|| PathBuf::from(value));
    }
    if let Some(rest) = value.strip_prefix("~/") {
        return paths::home_dir()
            .map(|home| home.join(rest))
            .unwrap_or_else(|| PathBuf::from(value));
    }
    if let Some(rest) = value.strip_prefix("$HOME/") {
        return paths::home_dir()
            .map(|home| home.join(rest))
            .unwrap_or_else(|| PathBuf::from(value));
    }
    if value == "$HOME" {
        return paths::home_dir().unwrap_or_else(|| PathBuf::from(value));
    }
    PathBuf::from(value)
}
