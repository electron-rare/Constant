mod buddy;
mod capabilities;
mod chat;
mod cockpit;
mod config;
mod executor;
mod fleet;
mod memory;
mod mission;
mod operator;
mod paths;
mod state;
mod tui;

use std::env;
use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::{thread, time::Duration};

use serde::Serialize;
use serde_json::{Value, json};

use buddy::buddy_ask;
use capabilities::{AGENTS, MINIMAL_STACK, SKILLS, VERSION, WORKFLOW_STACK};
use chat::delete_mission_thread;
use cockpit::{
    capture_pane, cockpit_attach, cockpit_doctor, cockpit_open, cockpit_status_line,
    focus_machine, restart_pane, runtime_status, send_to_pane,
};
use config::{fleet_machine, load_fleet_config, load_memory_config, load_models_config};
use executor::{run_mission, summarize_mission_command, verify_mission};
use fleet::{
    bridge_sync, fleet_check, fleet_config_query, render_scan_json, write_fleet_config,
};
use memory::{
    enroll_workspace, list_decisions, memory_status, persona_markdown, rebuild_workspace_memory,
    search_memory, summarize_mission_to_memory, sync_qdrant,
};
use mission::{delegate_step, plan_mission, retry_mission};
use operator::{health_value, models_status_value};
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
        "models" => handle_models(&args[1..]),
        "doctor" => handle_doctor(&args[1..]),
        "tui" => handle_tui(&args[1..]),
        "cockpit" => handle_cockpit(&args[1..]),
        "mission" => handle_mission(&args[1..]),
        "delegate" => handle_delegate(&args[1..]),
        "buddy" => handle_buddy(&args[1..]),
        "memory" => handle_memory(&args[1..]),
        "fleet" => handle_fleet(&args[1..]),
        other => Err(format!("unknown command: {other}")),
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
  models status [--json]
  agents [--json]
  skills [--json] [--public-only]
  tui [--workspace DIR] [--local-session NAME] [--session NAME]
  cockpit open|attach|status|status-line|doctor|focus|send|capture|restart
  mission create <prompt> [--workspace DIR] [--json]
  mission plan <mission_id> [--json]
  mission run <mission_id> [--json]
  mission delete <mission_id> [--json]
  mission status [mission_id] [--verbose] [--json]
  mission tail <mission_id> [--follow]
  mission verify <mission_id> [--step-id ID] [--json]
  mission retry <mission_id> [--step-id ID] [--json]
  mission summarize <mission_id> [--json]
  delegate <mission_id> [--step-id ID] [--machine LABEL] [--backend NAME] [--cli NAME] [--agent ID] [--skill ID] [--json]
  buddy ask <prompt> [--mission-id ID] [--json]
  memory status|rebuild|enroll|search|persona show|decisions|sync-qdrant

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
            other => return Err(format!("unknown skills option: {other}")),
        }
    }
    print_skills(as_json, public_only)?;
    Ok(ExitCode::SUCCESS)
}

fn handle_models(args: &[String]) -> Result<ExitCode, String> {
    if args.is_empty() {
        return Err("models requires a subcommand".to_string());
    }
    match args[0].as_str() {
        "status" => models_status_cmd(&args[1..]),
        other => Err(format!("unknown models subcommand: {other}")),
    }
}

fn handle_agents(args: &[String]) -> Result<ExitCode, String> {
    let mut as_json = false;
    for arg in args {
        match arg.as_str() {
            "--json" => as_json = true,
            other => return Err(format!("unknown agents option: {other}")),
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
            other => return Err(format!("unknown doctor option: {other}")),
        }
    }
    print_doctor(as_json)?;
    Ok(ExitCode::SUCCESS)
}

fn models_status_cmd(args: &[String]) -> Result<ExitCode, String> {
    let mut as_json = false;
    for arg in args {
        match arg.as_str() {
            "--json" => as_json = true,
            other => return Err(format!("unknown models status option: {other}")),
        }
    }
    print_value(&models_status_value()?, as_json)?;
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
            other => return Err(format!("unknown tui option: {other}")),
        }
        index += 1;
    }

    let workspace = canonical_workspace(&workspace)?;
    match run_tui(workspace.clone(), local_session.clone(), machine_session.clone())? {
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
        "status-line" => cockpit_status_line_cmd(&args[1..]),
        "doctor" => cockpit_doctor_cmd(&args[1..]),
        "focus" => cockpit_focus_cmd(&args[1..]),
        "send" => cockpit_send_cmd(&args[1..]),
        "capture" => cockpit_capture_cmd(&args[1..]),
        "restart" => cockpit_restart_cmd(&args[1..]),
        other => Err(format!("unknown cockpit subcommand: {other}")),
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
        "delete" => mission_delete_cmd(&args[1..]),
        "status" => mission_status(&args[1..]),
        "tail" => mission_tail(&args[1..]),
        "verify" => mission_verify_cmd(&args[1..]),
        "retry" => mission_retry_cmd(&args[1..]),
        "summarize" => mission_summarize_cmd(&args[1..]),
        other => Err(format!("unknown mission subcommand: {other}")),
    }
}

fn handle_buddy(args: &[String]) -> Result<ExitCode, String> {
    if args.is_empty() {
        return Err("buddy requires a subcommand".to_string());
    }
    match args[0].as_str() {
        "ask" => buddy_ask_cmd(&args[1..]),
        other => Err(format!("unknown buddy subcommand: {other}")),
    }
}

fn handle_memory(args: &[String]) -> Result<ExitCode, String> {
    if args.is_empty() {
        return Err("memory requires a subcommand".to_string());
    }
    match args[0].as_str() {
        "status" => memory_status_cmd(&args[1..]),
        "rebuild" => memory_rebuild_cmd(&args[1..]),
        "enroll" => memory_enroll_cmd(&args[1..]),
        "search" => memory_search_cmd(&args[1..]),
        "persona" => memory_persona_cmd(&args[1..]),
        "decisions" => memory_decisions_cmd(&args[1..]),
        "sync-qdrant" => memory_sync_qdrant_cmd(&args[1..]),
        other => Err(format!("unknown memory subcommand: {other}")),
    }
}

fn handle_fleet(args: &[String]) -> Result<ExitCode, String> {
    if args.is_empty() {
        return Err("fleet requires a subcommand".to_string());
    }
    match args[0].as_str() {
        "status" => fleet_status_cmd(&args[1..]),
        "sync" => fleet_sync_cmd(&args[1..]),
        "config-get" => fleet_config_get_cmd(&args[1..]),
        "render-scan-json" => fleet_render_scan_json_cmd(&args[1..]),
        "write-config" => fleet_write_config_cmd(&args[1..]),
        other => Err(format!("unknown fleet subcommand: {other}")),
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
            value if value.starts_with("--") => return Err(format!("unknown mission create option: {value}")),
            value => {
                if prompt.is_some() {
                    return Err("mission create accepts a single prompt".to_string());
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
            value if value.starts_with("--") => return Err(format!("unknown mission plan option: {value}")),
            value => {
                if mission_id.is_some() {
                    return Err("mission plan accepts a single mission_id".to_string());
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
            value if value.starts_with("--") => return Err(format!("unknown mission run option: {value}")),
            value => {
                if mission_id.is_some() {
                    return Err("mission run accepts a single mission_id".to_string());
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
            value if value.starts_with("--") => return Err(format!("unknown mission status option: {value}")),
            value => {
                if mission_id.is_some() {
                    return Err("mission status accepts at most one mission_id".to_string());
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
            value if value.starts_with("--") => return Err(format!("unknown mission tail option: {value}")),
            value => {
                if mission_id.is_some() {
                    return Err("mission tail accepts a single mission_id".to_string());
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
            value if value.starts_with("--") => return Err(format!("unknown mission verify option: {value}")),
            value => {
                if mission_id.is_some() {
                    return Err("mission verify accepts a single mission_id".to_string());
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
            value if value.starts_with("--") => return Err(format!("unknown mission retry option: {value}")),
            value => {
                if mission_id.is_some() {
                    return Err("mission retry accepts a single mission_id".to_string());
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
            value if value.starts_with("--") => return Err(format!("unknown mission summarize option: {value}")),
            value => {
                if mission_id.is_some() {
                    return Err("mission summarize accepts a single mission_id".to_string());
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

fn mission_delete_cmd(args: &[String]) -> Result<ExitCode, String> {
    let mut mission_id: Option<String> = None;
    let mut as_json = false;
    for arg in args {
        match arg.as_str() {
            "--json" => as_json = true,
            value if value.starts_with("--") => return Err(format!("unknown mission delete option: {value}")),
            value => {
                if mission_id.is_some() {
                    return Err("mission delete accepts a single mission_id".to_string());
                }
                mission_id = Some(value.to_string());
            }
        }
    }
    let mission_id = mission_id.ok_or_else(|| "mission delete requires a mission_id".to_string())?;
    delete_mission_thread(&mission_id)?;
    print_value(&json!({ "deleted": true, "mission_id": mission_id }), as_json)?;
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
            value if value.starts_with("--") => return Err(format!("unknown delegate option: {value}")),
            value => return Err(format!("unexpected delegate argument: {value}")),
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
            value if value.starts_with("--") => return Err(format!("unknown buddy ask option: {value}")),
            value => {
                if prompt.is_some() {
                    return Err("buddy ask accepts a single prompt".to_string());
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

fn memory_status_cmd(args: &[String]) -> Result<ExitCode, String> {
    let mut workspace: Option<String> = None;
    let mut as_json = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--workspace" => {
                index += 1;
                workspace = Some(
                    args.get(index)
                        .cloned()
                        .ok_or_else(|| "--workspace requires a value".to_string())?,
                );
            }
            "--json" => as_json = true,
            other => return Err(format!("unknown memory status option: {other}")),
        }
        index += 1;
    }
    print_value(&memory_status(workspace.as_deref())?, as_json)?;
    Ok(ExitCode::SUCCESS)
}

fn memory_rebuild_cmd(args: &[String]) -> Result<ExitCode, String> {
    let mut workspace = env::current_dir()
        .map_err(|err| format!("cannot resolve current directory: {err}"))?
        .to_string_lossy()
        .into_owned();
    let mut enroll = true;
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
            "--no-enroll" => enroll = false,
            "--json" => as_json = true,
            other => return Err(format!("unknown memory rebuild option: {other}")),
        }
        index += 1;
    }
    print_value(&rebuild_workspace_memory(&workspace, enroll)?, as_json)?;
    Ok(ExitCode::SUCCESS)
}

fn memory_enroll_cmd(args: &[String]) -> Result<ExitCode, String> {
    let mut path: Option<String> = None;
    let mut as_json = false;
    for arg in args {
        match arg.as_str() {
            "--json" => as_json = true,
            value if value.starts_with("--") => return Err(format!("unknown memory enroll option: {value}")),
            value => {
                if path.is_some() {
                    return Err("memory enroll accepts a single path".to_string());
                }
                path = Some(value.to_string());
            }
        }
    }
    let path = path.ok_or_else(|| "memory enroll requires a path".to_string())?;
    print_value(&enroll_workspace(&path)?, as_json)?;
    Ok(ExitCode::SUCCESS)
}

fn memory_search_cmd(args: &[String]) -> Result<ExitCode, String> {
    let mut query: Option<String> = None;
    let mut workspace: Option<String> = None;
    let mut limit: Option<usize> = None;
    let mut as_json = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--workspace" => {
                index += 1;
                workspace = Some(
                    args.get(index)
                        .cloned()
                        .ok_or_else(|| "--workspace requires a value".to_string())?,
                );
            }
            "--limit" => {
                index += 1;
                limit = Some(
                    args.get(index)
                        .ok_or_else(|| "--limit requires a value".to_string())?
                        .parse::<usize>()
                        .map_err(|_| "--limit must be an integer".to_string())?,
                );
            }
            "--json" => as_json = true,
            value if value.starts_with("--") => return Err(format!("unknown memory search option: {value}")),
            value => {
                if query.is_some() {
                    return Err("memory search accepts a single query".to_string());
                }
                query = Some(value.to_string());
            }
        }
        index += 1;
    }
    let query = query.ok_or_else(|| "memory search requires a query".to_string())?;
    print_value(&search_memory(&query, workspace.as_deref(), limit)?, as_json)?;
    Ok(ExitCode::SUCCESS)
}

fn memory_persona_cmd(args: &[String]) -> Result<ExitCode, String> {
    if args.is_empty() {
        return Err("memory persona requires a subcommand".to_string());
    }
    match args[0].as_str() {
        "show" => {
            let as_json = args.iter().skip(1).any(|arg| arg == "--json");
            if args.iter().skip(1).any(|arg| arg != "--json") {
                return Err("unknown memory persona show option".to_string());
            }
            let persona = persona_markdown()?;
            if as_json {
                print_value(&json!({ "persona": persona }), true)?;
            } else {
                println!("{persona}");
            }
            Ok(ExitCode::SUCCESS)
        }
        other => Err(format!("unknown memory persona subcommand: {other}")),
    }
}

fn memory_decisions_cmd(args: &[String]) -> Result<ExitCode, String> {
    let mut workspace: Option<String> = None;
    let mut mission_id: Option<String> = None;
    let mut as_json = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--workspace" => {
                index += 1;
                workspace = Some(
                    args.get(index)
                        .cloned()
                        .ok_or_else(|| "--workspace requires a value".to_string())?,
                );
            }
            "--mission-id" => {
                index += 1;
                mission_id = Some(
                    args.get(index)
                        .cloned()
                        .ok_or_else(|| "--mission-id requires a value".to_string())?,
                );
            }
            "--json" => as_json = true,
            other => return Err(format!("unknown memory decisions option: {other}")),
        }
        index += 1;
    }
    print_value(&list_decisions(workspace.as_deref(), mission_id.as_deref())?, as_json)?;
    Ok(ExitCode::SUCCESS)
}

fn memory_sync_qdrant_cmd(args: &[String]) -> Result<ExitCode, String> {
    let mut workspace: Option<String> = None;
    let mut as_json = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--workspace" => {
                index += 1;
                workspace = Some(
                    args.get(index)
                        .cloned()
                        .ok_or_else(|| "--workspace requires a value".to_string())?,
                );
            }
            "--json" => as_json = true,
            other => return Err(format!("unknown memory sync-qdrant option: {other}")),
        }
        index += 1;
    }
    let payload = sync_qdrant(workspace.as_deref())?;
    let ok = payload.get("ok").and_then(Value::as_bool).unwrap_or(false)
        || payload.get("skipped").and_then(Value::as_bool).unwrap_or(false);
    print_value(&payload, as_json)?;
    Ok(exit_code_from_status(Some(if ok { 0 } else { 1 })))
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

fn fleet_config_get_cmd(args: &[String]) -> Result<ExitCode, String> {
    if args.len() != 1 {
        return Err("fleet config-get requires exactly one query name".to_string());
    }
    if let Some(lines) = fleet_config_query(&args[0])? {
        for line in lines {
            println!("{line}");
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn fleet_render_scan_json_cmd(args: &[String]) -> Result<ExitCode, String> {
    if args.len() != 1 {
        return Err("fleet render-scan-json requires a candidates file".to_string());
    }
    let path = Path::new(&args[0]);
    print_value(&render_scan_json(path)?, true)?;
    Ok(ExitCode::SUCCESS)
}

fn fleet_write_config_cmd(args: &[String]) -> Result<ExitCode, String> {
    if args.is_empty() {
        return Err("fleet write-config requires a finalized selection file".to_string());
    }
    let finalized_file = Path::new(&args[0]);
    let mut output_path: Option<PathBuf> = None;
    let mut repo_dir: Option<String> = None;

    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--output" => {
                index += 1;
                output_path = Some(PathBuf::from(
                    args.get(index)
                        .ok_or_else(|| "--output requires a value".to_string())?,
                ));
            }
            "--repo-dir" => {
                index += 1;
                repo_dir = Some(
                    args.get(index)
                        .cloned()
                        .ok_or_else(|| "--repo-dir requires a value".to_string())?,
                );
            }
            other => return Err(format!("unknown fleet write-config option: {other}")),
        }
        index += 1;
    }

    let output_path = output_path.ok_or_else(|| "--output is required".to_string())?;
    let repo_dir = repo_dir.ok_or_else(|| "--repo-dir is required".to_string())?;
    let written = write_fleet_config(finalized_file, &output_path, &repo_dir)?;
    println!("{}", written.display());
    Ok(ExitCode::SUCCESS)
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
            other => return Err(format!("unknown cockpit open option: {other}")),
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
            other => return Err(format!("unknown cockpit attach option: {other}")),
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
            other => return Err(format!("unknown cockpit status option: {other}")),
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
            other => return Err(format!("unknown cockpit doctor option: {other}")),
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
            other => return Err(format!("unknown cockpit focus option: {other}")),
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
            other => return Err(format!("unknown cockpit send option: {other}")),
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
            other => return Err(format!("unknown cockpit capture option: {other}")),
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
            other => return Err(format!("unknown cockpit restart option: {other}")),
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

fn cockpit_status_line_cmd(args: &[String]) -> Result<ExitCode, String> {
    let mut workspace = env::current_dir()
        .map_err(|err| format!("cannot resolve current directory: {err}"))?
        .to_string_lossy()
        .into_owned();
    let mut scope_label: Option<String> = None;
    let mut machine_label: Option<String> = None;
    let mut max_length = 180_usize;
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
            "--scope-label" => {
                index += 1;
                scope_label = Some(
                    args.get(index)
                        .cloned()
                        .ok_or_else(|| "--scope-label requires a value".to_string())?,
                );
            }
            "--machine-label" => {
                index += 1;
                machine_label = Some(
                    args.get(index)
                        .cloned()
                        .ok_or_else(|| "--machine-label requires a value".to_string())?,
                );
            }
            "--max-length" => {
                index += 1;
                max_length = args
                    .get(index)
                    .ok_or_else(|| "--max-length requires a value".to_string())?
                    .parse::<usize>()
                    .map_err(|_| "--max-length must be an integer".to_string())?;
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
            other => return Err(format!("unknown cockpit status-line option: {other}")),
        }
        index += 1;
    }
    println!(
        "{}",
        cockpit_status_line(
            &canonical_workspace(&workspace)?,
            scope_label.as_deref(),
            machine_label.as_deref(),
            max_length,
            &local_session,
            &machine_session,
        )?
    );
    Ok(ExitCode::SUCCESS)
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
    let health = health_value()?;

    Ok(json!({
        "version": VERSION,
        "repo_root": paths::repo_root().display().to_string(),
        "cache_root": paths::cache_root().display().to_string(),
        "config_root": paths::config_root().display().to_string(),
        "data_root": paths::data_root().display().to_string(),
        "rust": {
            "front_controller": true,
            "python_handoff": false,
        },
        "commands": {
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
        "health": health,
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
        "mode": "rust-only",
        "last_mode": read_wrapper_mode(&wrapper_dir.join("last-mode")),
        "codesign_verify": if rust_bin.exists() { Some(run_quick_command(&["codesign", "--verify", "--verbose=2", &rust_bin.display().to_string()])) } else { None },
        "spctl_assess": if rust_bin.exists() { Some(run_quick_command(&["spctl", "--assess", "-vv", &rust_bin.display().to_string()])) } else { None },
        "xattr_provenance": if rust_bin.exists() { Some(run_quick_command(&["xattr", "-p", "com.apple.provenance", &rust_bin.display().to_string()])) } else { None },
        "hint": "The public wrapper is Rust-only. If the binary is missing or stale, rerun ./scripts/Constant and it will rebuild and ad-hoc sign the binary before launch.",
    })
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
