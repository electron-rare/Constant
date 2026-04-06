use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::config::{FleetConfig, MachineConfig, read_fleet_config_if_present, write_fleet_config_file};
use crate::paths;

pub fn fleet_check() -> Result<Value, String> {
    let script = paths::repo_root().join("scripts/constant-fleet-install.sh");
    let output = std::process::Command::new(script)
        .arg("check")
        .output()
        .map_err(|err| format!("cannot run fleet check: {err}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let mut machines = Vec::new();
    let mut current = serde_json::Map::new();
    for raw_line in stdout.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with("===") && line.contains('(') {
            if !current.is_empty() {
                machines.push(Value::Object(current.clone()));
                current.clear();
            }
            let label = line.split_whitespace().nth(1).unwrap_or_default();
            current.insert("label".to_string(), Value::String(label.to_string()));
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            current.insert(key.to_string(), Value::String(value.to_string()));
        }
    }
    if !current.is_empty() {
        machines.push(Value::Object(current));
    }

    Ok(json!({
        "returncode": output.status.code().unwrap_or(1),
        "machines": machines,
        "stderr": stderr.trim(),
    }))
}

pub fn bridge_sync() -> Result<Value, String> {
    let script = paths::repo_root().join("scripts/ai-bridge.sh");
    let output = std::process::Command::new(script)
        .arg("sync")
        .output()
        .map_err(|err| format!("cannot run bridge sync: {err}"))?;
    Ok(json!({
        "returncode": output.status.code().unwrap_or(1),
        "stdout": String::from_utf8_lossy(&output.stdout),
        "stderr": String::from_utf8_lossy(&output.stderr),
    }))
}

pub fn fleet_config_query(expr: &str) -> Result<Option<Vec<String>>, String> {
    let Some(config) = read_fleet_config_if_present()? else {
        return Ok(None);
    };

    let values = match expr {
        "repo_dir" => vec![config.repo_dir],
        "local_machine" => vec![config.local_machine],
        "machine_specs" => config
            .machines
            .into_iter()
            .map(|machine| format!("{}={}", machine.label, machine.target))
            .collect(),
        other => return Err(format!("unknown fleet config query: {other}")),
    };

    Ok(Some(values))
}

pub fn render_scan_json(path: &Path) -> Result<Value, String> {
    let text =
        fs::read_to_string(path).map_err(|err| format!("cannot read {}: {err}", path.display()))?;
    let mut candidates = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() != 9 {
            return Err(format!(
                "cannot parse {}: expected 9 tab-separated fields, got {}",
                path.display(),
                fields.len()
            ));
        }
        candidates.push(json!({
            "seed": fields[0],
            "user": fields[1],
            "host": fields[2],
            "port": fields[3].parse::<u16>().map_err(|_| {
                format!("cannot parse port '{}' in {}", fields[3], path.display())
            })?,
            "reachable": fields[4] == "yes",
            "remote_name": fields[5],
            "remote_os": fields[6],
            "remote_home": fields[7],
            "error": fields[8],
        }));
    }
    Ok(json!({ "candidates": candidates }))
}

pub fn write_fleet_config(
    finalized_file: &Path,
    output_path: &Path,
    repo_dir: &str,
) -> Result<PathBuf, String> {
    let text = fs::read_to_string(finalized_file)
        .map_err(|err| format!("cannot read {}: {err}", finalized_file.display()))?;
    let mut machines = Vec::new();
    let mut local_machine: Option<String> = None;

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() != 4 {
            return Err(format!(
                "cannot parse {}: expected 4 tab-separated fields, got {}",
                finalized_file.display(),
                fields.len()
            ));
        }
        let label = fields[0].to_string();
        let role = fields[1];
        let user = fields[2];
        let seed = fields[3];

        if role == "local" {
            local_machine = Some(label.clone());
            machines.push(MachineConfig {
                label,
                target: "local".to_string(),
                auto_clis: vec!["codex".into(), "vibe".into(), "claude".into()],
                manual_clis: vec!["copilot".into()],
                backends: vec!["omc".into(), "cli-local".into(), "cockpit".into()],
            });
        } else {
            machines.push(MachineConfig {
                label,
                target: format!("{user}@{seed}"),
                auto_clis: vec!["codex".into(), "vibe".into(), "claude".into()],
                manual_clis: vec!["copilot".into()],
                backends: vec!["cli-ssh".into(), "cockpit".into()],
            });
        }
    }

    let config = FleetConfig {
        version: 1,
        local_machine: local_machine.unwrap_or_else(|| "command-center".to_string()),
        repo_dir: repo_dir.to_string(),
        machines,
    };

    let resolved_output = if output_path.is_absolute() {
        output_path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|err| format!("cannot resolve current directory: {err}"))?
            .join(output_path)
    };
    write_fleet_config_file(&resolved_output, &config)?;
    Ok(resolved_output)
}
