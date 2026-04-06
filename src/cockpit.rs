use std::process::{Command, ExitStatus};

use serde::Serialize;
use serde_json::{Value, json};

use crate::config::{MachineConfig, fleet_machine, load_fleet_config};
use crate::paths;

pub const ROLES: [&str; 4] = ["claude", "codex", "copilot", "vibe"];

#[derive(Debug, Clone, Serialize)]
pub struct PaneStatus {
    pub session_name: String,
    pub window_name: String,
    pub pane_id: String,
    pub pane_index: u32,
    pub role: String,
    pub pane_role: String,
    pub pane_title: String,
    pub pane_command: String,
    pub active: bool,
    pub dead: bool,
    pub dead_status: i32,
    pub autorestart_failures: u32,
    pub autorestart_disabled: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct MachineRuntimeStatus {
    pub label: String,
    pub target: String,
    pub session: String,
    pub reachable: bool,
    pub attached_window: String,
    pub session_exists: bool,
    pub panes: Vec<PaneStatus>,
    pub roles: std::collections::BTreeMap<String, Option<PaneStatus>>,
    pub stderr: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FleetRuntimeStatus {
    pub local_session: String,
    pub machine_session: String,
    pub fleet_session_exists: bool,
    pub fleet_windows: Vec<String>,
    pub focused_machine: Option<String>,
    pub focused_role: Option<String>,
    pub machines: Vec<MachineRuntimeStatus>,
    pub fleet_stderr: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CommandResult {
    pub argv: Vec<String>,
    pub returncode: i32,
    pub stdout: String,
    pub stderr: String,
}

pub fn runtime_status(
    local_session: &str,
    machine_session: &str,
) -> Result<FleetRuntimeStatus, String> {
    let fleet = load_fleet_config()?;
    let local_tmux = run_command(
        &[
            "tmux".to_string(),
            "list-windows".to_string(),
            "-t".to_string(),
            local_session.to_string(),
            "-F".to_string(),
            "#{window_name}\t#{window_active}".to_string(),
        ],
        None,
    )?;
    let mut fleet_windows = Vec::new();
    let mut focused_machine = None;

    if local_tmux.returncode == 0 {
        for raw in local_tmux.stdout.lines() {
            let mut parts = raw.split('\t');
            let window_name = parts.next().unwrap_or_default().to_string();
            let active = parts.next().unwrap_or("0");
            if !window_name.is_empty() {
                if active == "1" {
                    focused_machine = Some(window_name.clone());
                }
                fleet_windows.push(window_name);
            }
        }
    }

    let machines = fleet
        .machines
        .iter()
        .map(|machine| machine_tmux_status(machine, machine_session))
        .collect::<Result<Vec<_>, _>>()?;

    let mut focused_role = None;
    if let Some(machine_name) = focused_machine.as_deref() {
        if let Some(machine) = machines
            .iter()
            .find(|machine| machine.label == machine_name)
        {
            for role in ROLES {
                if machine
                    .roles
                    .get(role)
                    .and_then(|pane| pane.as_ref())
                    .map(|pane| pane.active)
                    .unwrap_or(false)
                {
                    focused_role = Some(role.to_string());
                    break;
                }
            }
        }
    }

    Ok(FleetRuntimeStatus {
        local_session: local_session.to_string(),
        machine_session: machine_session.to_string(),
        fleet_session_exists: local_tmux.returncode == 0,
        fleet_windows,
        focused_machine,
        focused_role,
        machines,
        fleet_stderr: local_tmux.stderr.trim().to_string(),
    })
}

pub fn cockpit_doctor(local_session: &str, machine_session: &str) -> Result<Value, String> {
    let tmux = run_command(&["tmux".to_string(), "-V".to_string()], None)?;
    let payload = runtime_status(local_session, machine_session)?;
    Ok(json!({
        "tmux": {
            "available": tmux.returncode == 0,
            "stdout": tmux.stdout.trim(),
            "stderr": tmux.stderr.trim(),
        },
        "status": payload,
    }))
}

pub fn cockpit_status_line(
    workspace: &str,
    scope_label: Option<&str>,
    machine_label: Option<&str>,
    max_length: usize,
    local_session: &str,
    machine_session: &str,
) -> Result<String, String> {
    let chat_line =
        crate::chat::render_chat_dock_line(workspace, scope_label, machine_label, max_length)?;
    let runtime = runtime_status(local_session, machine_session)?;
    if let Some(warning) = autorestart_warning(&runtime) {
        return Ok(clip_line(&format!("{chat_line} | {warning}"), max_length));
    }
    Ok(chat_line)
}

pub fn cockpit_open(
    workspace: &str,
    local_session: &str,
    machine_session: &str,
    recreate: bool,
    remote_recreate: bool,
) -> Result<i32, String> {
    let script = paths::repo_root().join("scripts/constant-fleet.sh");
    let mut args = vec![
        script.display().to_string(),
        "--workspace".to_string(),
        workspace.to_string(),
        "--local-session".to_string(),
        local_session.to_string(),
        "--session".to_string(),
        machine_session.to_string(),
    ];
    if recreate {
        args.push("--recreate".to_string());
    }
    if remote_recreate {
        args.push("--remote-recreate".to_string());
    }
    Ok(run_status(&args, None)?.code().unwrap_or(1))
}

pub fn cockpit_attach(local_session: &str) -> Result<i32, String> {
    let script = paths::repo_root().join("scripts/constant-fleet.sh");
    let args = vec![
        script.display().to_string(),
        "--attach-only".to_string(),
        "--local-session".to_string(),
        local_session.to_string(),
    ];
    Ok(run_status(&args, None)?.code().unwrap_or(1))
}

pub fn focus_machine(
    machine_label: &str,
    pane_role: Option<&str>,
    local_session: &str,
    machine_session: &str,
) -> Result<CommandResult, String> {
    let fleet = load_fleet_config()?;
    let machine = fleet_machine(&fleet, machine_label)?;
    let script = paths::repo_root().join("scripts/constant-machine.sh");

    if let Some(role) = pane_role {
        let result = run_machine_command(
            machine,
            &[
                script.display().to_string(),
                "--session".to_string(),
                machine_session.to_string(),
                "--focus-pane".to_string(),
                role.to_string(),
            ],
        )?;
        if result.returncode != 0 {
            return Ok(result);
        }
    }

    run_command(
        &[
            "tmux".to_string(),
            "select-window".to_string(),
            "-t".to_string(),
            format!("{local_session}:{machine_label}"),
        ],
        None,
    )
}

pub fn send_to_pane(
    machine_label: &str,
    pane_role: &str,
    command: &str,
    machine_session: &str,
) -> Result<CommandResult, String> {
    let fleet = load_fleet_config()?;
    let machine = fleet_machine(&fleet, machine_label)?;
    let script = paths::repo_root().join("scripts/constant-machine.sh");
    run_machine_command(
        machine,
        &[
            script.display().to_string(),
            "--session".to_string(),
            machine_session.to_string(),
            "--send-pane".to_string(),
            pane_role.to_string(),
            "--command".to_string(),
            command.to_string(),
        ],
    )
}

pub fn capture_pane(
    machine_label: &str,
    pane_role: &str,
    lines: u32,
    machine_session: &str,
) -> Result<CommandResult, String> {
    let fleet = load_fleet_config()?;
    let machine = fleet_machine(&fleet, machine_label)?;
    let script = paths::repo_root().join("scripts/constant-machine.sh");
    run_machine_command(
        machine,
        &[
            script.display().to_string(),
            "--session".to_string(),
            machine_session.to_string(),
            "--capture-pane".to_string(),
            pane_role.to_string(),
            "--lines".to_string(),
            lines.to_string(),
        ],
    )
}

pub fn restart_pane(
    machine_label: &str,
    pane_role: &str,
    machine_session: &str,
) -> Result<CommandResult, String> {
    let fleet = load_fleet_config()?;
    let machine = fleet_machine(&fleet, machine_label)?;
    let script = paths::repo_root().join("scripts/constant-machine.sh");
    run_machine_command(
        machine,
        &[
            script.display().to_string(),
            "--session".to_string(),
            machine_session.to_string(),
            "--restart-pane".to_string(),
            pane_role.to_string(),
        ],
    )
}

fn machine_tmux_status(
    machine: &MachineConfig,
    session: &str,
) -> Result<MachineRuntimeStatus, String> {
    let label = machine.label.clone();
    let session_target = format!("{session}:{label}");
    let result = if is_local_target(&machine.target) {
        run_command(&tmux_list_command(&session_target), None)?
    } else {
        let inner = shlex_join(&tmux_list_command(&session_target));
        run_command(&ssh_command(&machine.target, &inner), None)?
    };

    let panes = if result.returncode == 0 {
        parse_panes(&result.stdout)
    } else {
        Vec::new()
    };
    let mut roles = std::collections::BTreeMap::new();
    for role in ROLES {
        roles.insert(
            role.to_string(),
            panes.iter().find(|pane| pane.role == role).cloned(),
        );
    }

    Ok(MachineRuntimeStatus {
        label: machine.label.clone(),
        target: machine.target.clone(),
        session: session_target,
        reachable: result.returncode == 0 || is_local_target(&machine.target),
        attached_window: machine.label.clone(),
        session_exists: result.returncode == 0,
        panes,
        roles,
        stderr: result.stderr.trim().to_string(),
    })
}

fn parse_panes(stdout: &str) -> Vec<PaneStatus> {
    let mut panes = stdout
        .lines()
        .filter_map(|raw| {
            let parts = raw.split('\t').collect::<Vec<_>>();
            if parts.len() != 12 {
                return None;
            }
            let role = if !parts[4].is_empty() {
                parts[4].to_string()
            } else if ROLES.contains(&parts[5]) {
                parts[5].to_string()
            } else {
                parts[6].to_string()
            };
            Some(PaneStatus {
                session_name: parts[0].to_string(),
                window_name: parts[1].to_string(),
                pane_id: parts[2].to_string(),
                pane_index: parts[3].parse().unwrap_or(0),
                role,
                pane_role: parts[4].to_string(),
                pane_title: parts[5].to_string(),
                pane_command: parts[6].to_string(),
                active: parts[7] == "1",
                dead: parts[8] == "1",
                dead_status: parts[9].parse().unwrap_or(0),
                autorestart_failures: parts[10].parse().unwrap_or(0),
                autorestart_disabled: parts[11] == "1",
            })
        })
        .collect::<Vec<_>>();
    panes.sort_by_key(|pane| pane.pane_index);
    panes
}

fn tmux_list_command(session_name: &str) -> Vec<String> {
    vec![
        "tmux".to_string(),
        "list-panes".to_string(),
        "-t".to_string(),
        session_name.to_string(),
        "-F".to_string(),
        "#{session_name}\t#{window_name}\t#{pane_id}\t#{pane_index}\t#{@constant_role}\t#{pane_title}\t#{pane_current_command}\t#{pane_active}\t#{pane_dead}\t#{pane_dead_status}\t#{@constant_restart_failures}\t#{@constant_autorestart_disabled}".to_string(),
    ]
}

fn autorestart_warning(runtime: &FleetRuntimeStatus) -> Option<String> {
    for machine in &runtime.machines {
        for pane in &machine.panes {
            if pane.autorestart_disabled {
                return Some(format!(
                    "respawn disabled after {} failures: {}",
                    pane.autorestart_failures.max(1),
                    pane.role
                ));
            }
        }
    }
    None
}

fn clip_line(line: &str, max_length: usize) -> String {
    if line.len() <= max_length {
        return line.to_string();
    }
    format!(
        "{}...",
        line.chars()
            .take(max_length.saturating_sub(3))
            .collect::<String>()
            .trim_end()
    )
}

fn ssh_command(target: &str, inner: &str) -> Vec<String> {
    vec![
        "ssh".to_string(),
        "-o".to_string(),
        "BatchMode=yes".to_string(),
        "-o".to_string(),
        "ConnectTimeout=1".to_string(),
        target.to_string(),
        format!(
            "PATH=\"$HOME/.local/bin:$HOME/.npm-global/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin\"; export PATH; {inner}"
        ),
    ]
}

fn run_machine_command(machine: &MachineConfig, args: &[String]) -> Result<CommandResult, String> {
    let mut with_env = vec![
        "env".to_string(),
        format!("ZELLIJ_AI_MACHINE_NAME={}", machine.label),
    ];
    with_env.extend(args.iter().cloned());
    if is_local_target(&machine.target) {
        return run_command(&with_env, None);
    }
    let quoted = shlex_join(&with_env);
    run_command(&ssh_command(&machine.target, &quoted), None)
}

fn run_command(args: &[String], cwd: Option<&str>) -> Result<CommandResult, String> {
    let mut command = Command::new(&args[0]);
    command.args(&args[1..]);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let output = command
        .output()
        .map_err(|err| format!("command failed to start {}: {err}", args[0]))?;
    Ok(CommandResult {
        argv: args.to_vec(),
        returncode: output.status.code().unwrap_or(1),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    })
}

fn run_status(args: &[String], cwd: Option<&str>) -> Result<ExitStatus, String> {
    let mut command = Command::new(&args[0]);
    command.args(&args[1..]);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    command
        .status()
        .map_err(|err| format!("command failed to start {}: {err}", args[0]))
}

fn is_local_target(target: &str) -> bool {
    if matches!(target, "local" | "localhost" | "127.0.0.1" | "::1") {
        return true;
    }
    let hostname = std::env::var("HOSTNAME").unwrap_or_default();
    let short = hostname.split('.').next().unwrap_or_default();
    target == hostname || target == short
}

fn shlex_join(args: &[String]) -> String {
    args.iter()
        .map(|arg| shell_quote(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }
    if value.chars().all(|ch| {
        ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '/' | '.' | ':' | '@' | '=')
    }) {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}
