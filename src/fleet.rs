use serde_json::{Value, json};

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
