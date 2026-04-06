use std::process::{Command, Stdio};
use std::time::Instant;

use serde_json::{Value, json};

use crate::config::{FleetConfig, fleet_machine};
use crate::memory::summarize_mission_to_memory;
use crate::mission::plan_mission;
use crate::state::{
    Mission, MissionStep, append_event, load_mission, mission_summary_value, save_mission,
};

pub fn run_mission(mission: &mut Mission, fleet: &FleetConfig) -> Result<(Value, bool), String> {
    if mission.steps.is_empty() {
        plan_mission(mission, fleet)?;
        save_mission(mission)?;
        append_event(
            &mission.mission_id,
            "mission.planned",
            json!({
                "plan": {
                    "title": mission.title,
                    "summary": mission.planner_summary,
                    "steps": mission.steps,
                },
                "buddy_review": mission.buddy_review,
            }),
        )?;
    }

    loop {
        let Some(step_index) = first_active_step_index(mission) else {
            mission.status = "done".to_string();
            save_mission(mission)?;
            append_event(
                &mission.mission_id,
                "mission.done",
                json!({"mission_id": mission.mission_id}),
            )?;
            return Ok((mission_summary_value(mission), true));
        };

        let step_snapshot = mission
            .steps
            .get(step_index)
            .cloned()
            .ok_or_else(|| "active step disappeared".to_string())?;

        {
            let step = mission
                .steps
                .get_mut(step_index)
                .ok_or_else(|| "active step disappeared".to_string())?;
            step.status = "running".to_string();
            step.attempt += 1;
        }
        mission.status = "running".to_string();
        save_mission(mission)?;
        append_event(
            &mission.mission_id,
            "step.started",
            json!({
                "step_id": step_snapshot.step_id,
                "machine": step_snapshot.machine,
                "backend": step_snapshot.backend,
                "cli": step_snapshot.cli,
            }),
        )?;

        let execution = execute_step(&step_snapshot, mission, fleet)?;
        let artifact_path = artifact_for_attempt(mission, step_index, &execution)?;
        append_event(
            &mission.mission_id,
            "step.executed",
            json!({
                "step_id": step_snapshot.step_id,
                "artifact": artifact_path,
                "returncode": execution["returncode"],
            }),
        )?;

        let verdict = verify_execution(mission, &step_snapshot, &execution);
        let decision = verdict
            .get("decision")
            .and_then(Value::as_str)
            .unwrap_or("failed");

        {
            let step = mission
                .steps
                .get_mut(step_index)
                .ok_or_else(|| "active step disappeared".to_string())?;
            step.artifact_refs.push(artifact_path.clone());
            step.result_summary = verdict
                .get("summary")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            step.extra
                .insert("verification".to_string(), verdict.clone());

            match decision {
                "done" => step.status = "done".to_string(),
                "retry" if step.attempt < 2 => step.status = "pending".to_string(),
                "needs_human" => step.status = "needs_human".to_string(),
                _ => step.status = "failed".to_string(),
            }
        }

        match decision {
            "done" => {
                save_mission(mission)?;
                append_event(
                    &mission.mission_id,
                    "step.verified",
                    json!({"step_id": step_snapshot.step_id, "verdict": verdict}),
                )?;
            }
            "retry" => {
                mission.status = "planned".to_string();
                save_mission(mission)?;
                append_event(
                    &mission.mission_id,
                    "step.verified",
                    json!({"step_id": step_snapshot.step_id, "verdict": verdict}),
                )?;
            }
            "needs_human" => {
                mission.status = "needs_human".to_string();
                save_mission(mission)?;
                append_event(
                    &mission.mission_id,
                    "step.needs_human",
                    json!({"step_id": step_snapshot.step_id, "summary": verdict["summary"]}),
                )?;
                let summary = summarize_mission(mission)?;
                return Ok((json!({"summary": summary, "verdict": verdict}), false));
            }
            _ => {
                mission.status = "failed".to_string();
                save_mission(mission)?;
                append_event(
                    &mission.mission_id,
                    "step.failed",
                    json!({"step_id": step_snapshot.step_id, "summary": verdict["summary"]}),
                )?;
                let summary = summarize_mission(mission)?;
                return Ok((json!({"summary": summary, "verdict": verdict}), false));
            }
        }
    }
}

pub fn verify_mission(
    mission: &mut Mission,
    step_id: Option<&str>,
) -> Result<(Value, bool), String> {
    let step_index = if let Some(step_id) = step_id {
        mission
            .steps
            .iter()
            .position(|entry| entry.step_id == step_id)
    } else if mission.steps.is_empty() {
        None
    } else {
        Some(mission.steps.len() - 1)
    };

    let Some(step_index) = step_index else {
        return Err("No step artifact available to verify.".to_string());
    };

    let step_snapshot = mission
        .steps
        .get(step_index)
        .cloned()
        .ok_or_else(|| "step not found".to_string())?;

    let artifact_path = step_snapshot
        .artifact_refs
        .last()
        .cloned()
        .ok_or_else(|| "No step artifact available to verify.".to_string())?;
    let artifact_text = std::fs::read_to_string(&artifact_path)
        .map_err(|err| format!("cannot read {artifact_path}: {err}"))?;
    let execution: Value = serde_json::from_str(&artifact_text)
        .map_err(|err| format!("cannot parse {artifact_path}: {err}"))?;
    let verdict = verify_execution(mission, &step_snapshot, &execution);

    if let Some(step) = mission.steps.get_mut(step_index) {
        step.result_summary = verdict
            .get("summary")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        step.extra
            .insert("verification".to_string(), verdict.clone());
    }
    save_mission(mission)?;
    append_event(
        &mission.mission_id,
        "mission.verify",
        json!({"step_id": step_snapshot.step_id, "verdict": verdict}),
    )?;
    let done = verdict
        .get("decision")
        .and_then(Value::as_str)
        .map(|value| value == "done")
        .unwrap_or(false);
    Ok((
        json!({"step_id": step_snapshot.step_id, "verdict": verdict}),
        done,
    ))
}

pub fn summarize_mission(mission: &mut Mission) -> Result<Value, String> {
    let status_mix = status_mix(mission);
    let status_mix_text = status_mix
        .iter()
        .map(|(key, value)| format!("{key}={}", value.as_u64().unwrap_or(0)))
        .collect::<Vec<_>>()
        .join(", ");
    let routes = mission
        .steps
        .iter()
        .map(|step| {
            format!(
                "{}={}/{}/{}:{}",
                step.step_id, step.machine, step.cli, step.backend, step.status
            )
        })
        .collect::<Vec<_>>();
    let summary = format!(
        "Mission {} ended as {}. Steps={}. Status mix={}. Routes: {}.",
        mission.title,
        mission.status,
        mission.steps.len(),
        status_mix_text,
        if routes.is_empty() {
            "none".to_string()
        } else {
            routes.join("; ")
        }
    );

    let artifact_name = format!("summary-{}.json", mission.mission_id);
    let artifact_payload = json!({
        "mission_id": mission.mission_id,
        "title": mission.title,
        "status": mission.status,
        "summary": summary,
        "status_mix": status_mix,
        "routes": routes,
    });
    let artifact_path = summary_artifact_path(mission, &artifact_name, &artifact_payload)?;
    mission.artifacts.push(artifact_path.clone());
    let memory_summary = summarize_mission_to_memory(mission)?;
    save_mission(mission)?;
    append_event(
        &mission.mission_id,
        "mission.summarized",
        json!({
            "artifact": artifact_payload.clone(),
            "memory": memory_summary,
        }),
    )?;

    Ok(json!({
        "mission_id": mission.mission_id,
        "title": mission.title,
        "status": mission.status,
        "summary": summary,
        "artifact": artifact_path,
        "status_mix": status_mix,
        "routes": routes,
        "memory": memory_summary,
    }))
}

pub fn execute_step(
    step: &MissionStep,
    mission: &Mission,
    fleet: &FleetConfig,
) -> Result<Value, String> {
    let machine = fleet_machine(fleet, &step.machine)?;
    if step.cli == "copilot" {
        return Ok(json!({
            "argv": [],
            "returncode": 1,
            "stdout": "",
            "stderr": "copilot is manual-only in Constant v1",
            "duration_s": 0.0,
        }));
    }

    if matches!(step.backend.as_str(), "cockpit" | "zellij") {
        let cockpit = crate::paths::repo_root().join("scripts/constant-fleet.sh");
        return Ok(json!({
            "argv": [cockpit.display().to_string(), "--workspace", mission.workspace],
            "returncode": 0,
            "stdout": format!("Open cockpit manually with: {} --workspace {}", cockpit.display(), mission.workspace),
            "stderr": "",
            "duration_s": 0.0,
        }));
    }

    let command = build_local_command(step, &mission.workspace)?;
    if matches!(step.backend.as_str(), "cli-local" | "omc") {
        return run_command(&command, Some(&mission.workspace));
    }
    if step.backend == "cli-ssh" {
        let quoted = shlex_join(&command);
        let remote_shell = format!(
            "export PATH=\"$HOME/.local/bin:$HOME/.npm-global/bin:$PATH\"; cd {} && {}",
            shell_quote(&mission.workspace),
            quoted
        );
        let ssh_cmd = vec![
            "ssh".to_string(),
            machine.target.clone(),
            "bash".to_string(),
            "-lc".to_string(),
            remote_shell,
        ];
        return run_command(&ssh_cmd, None);
    }

    Err(format!("Unsupported backend: {}", step.backend))
}

pub fn verify_execution(mission: &Mission, step: &MissionStep, execution: &Value) -> Value {
    let returncode = execution
        .get("returncode")
        .and_then(Value::as_i64)
        .unwrap_or(1);
    let stdout = execution
        .get("stdout")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let stderr = execution
        .get("stderr")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let combined = format!("{stdout}\n{stderr}").to_lowercase();

    let (decision, summary, confidence) = if returncode == 0
        && !combined.contains("open cockpit manually")
        && !combined.contains("manual-only")
    {
        (
            "done",
            format!(
                "Step {} completed successfully on {} with {}.",
                step.step_id, step.machine, step.cli
            ),
            "medium",
        )
    } else if combined.contains("open cockpit manually")
        || combined.contains("manual-only")
        || combined.contains("device-auth")
        || combined.contains("login")
        || combined.contains("auth")
    {
        (
            "needs_human",
            format!(
                "Step {} needs human attention: {}",
                step.step_id,
                first_nonempty_line(stderr, stdout)
            ),
            "high",
        )
    } else if step.attempt < 1 {
        (
            "retry",
            format!(
                "Step {} failed once and should retry: {}",
                step.step_id,
                first_nonempty_line(stderr, stdout)
            ),
            "medium",
        )
    } else {
        (
            "failed",
            format!(
                "Step {} failed after retry budget: {}",
                step.step_id,
                first_nonempty_line(stderr, stdout)
            ),
            "high",
        )
    };

    json!({
        "decision": decision,
        "summary": summary,
        "confidence": confidence,
        "mission_status": mission.status,
        "machine": step.machine,
        "cli": step.cli,
        "backend": step.backend,
    })
}

fn build_local_command(step: &MissionStep, workspace: &str) -> Result<Vec<String>, String> {
    match step.backend.as_str() {
        "omc" => {
            if !matches!(step.cli.as_str(), "claude" | "codex") {
                return Err(format!(
                    "OMC backend only supports claude/codex in v1, not {}",
                    step.cli
                ));
            }
            Ok(vec![
                "omc".to_string(),
                "ask".to_string(),
                step.cli.clone(),
                "--print".to_string(),
                step.prompt.clone(),
            ])
        }
        _ if step.cli == "claude" => Ok(vec![
            "claude".to_string(),
            "-p".to_string(),
            "--output-format".to_string(),
            "json".to_string(),
            "--permission-mode".to_string(),
            "acceptEdits".to_string(),
            step.prompt.clone(),
        ]),
        _ if step.cli == "codex" => Ok(vec![
            "codex".to_string(),
            "exec".to_string(),
            "--json".to_string(),
            "--full-auto".to_string(),
            "--skip-git-repo-check".to_string(),
            "-C".to_string(),
            workspace.to_string(),
            step.prompt.clone(),
        ]),
        _ if step.cli == "vibe" => Ok(vec![
            "vibe".to_string(),
            "-p".to_string(),
            step.prompt.clone(),
            "--output".to_string(),
            "json".to_string(),
            "--workdir".to_string(),
            workspace.to_string(),
        ]),
        _ => Err(format!("Unsupported auto CLI: {}", step.cli)),
    }
}

fn run_command(args: &[String], cwd: Option<&str>) -> Result<Value, String> {
    let started = Instant::now();
    let mut command = Command::new(&args[0]);
    command.args(&args[1..]);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    command.stdin(Stdio::null());
    let output = command
        .output()
        .map_err(|err| format!("command failed to start {}: {err}", args[0]))?;
    Ok(json!({
        "argv": args,
        "returncode": output.status.code().unwrap_or(1),
        "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
        "stderr": String::from_utf8_lossy(&output.stderr).to_string(),
        "duration_s": started.elapsed().as_secs_f64(),
    }))
}

fn artifact_for_attempt(
    mission: &mut Mission,
    step_index: usize,
    execution: &Value,
) -> Result<String, String> {
    let step = mission
        .steps
        .get(step_index)
        .ok_or_else(|| "step not found".to_string())?;
    let artifact_dir = crate::paths::mission_artifacts_dir(&mission.mission_id);
    std::fs::create_dir_all(&artifact_dir)
        .map_err(|err| format!("cannot create {}: {err}", artifact_dir.display()))?;
    let path = artifact_dir.join(format!("{}-attempt-{}.json", step.step_id, step.attempt));
    let text = serde_json::to_string_pretty(execution)
        .map_err(|err| format!("cannot encode artifact: {err}"))?;
    std::fs::write(&path, format!("{text}\n"))
        .map_err(|err| format!("cannot write {}: {err}", path.display()))?;
    let artifact_path = path.display().to_string();
    mission.artifacts.push(artifact_path.clone());
    Ok(artifact_path)
}

fn summary_artifact_path(mission: &Mission, name: &str, payload: &Value) -> Result<String, String> {
    let artifact_dir = crate::paths::mission_artifacts_dir(&mission.mission_id);
    std::fs::create_dir_all(&artifact_dir)
        .map_err(|err| format!("cannot create {}: {err}", artifact_dir.display()))?;
    let path = artifact_dir.join(name);
    let text = serde_json::to_string_pretty(payload)
        .map_err(|err| format!("cannot encode summary artifact: {err}"))?;
    std::fs::write(&path, format!("{text}\n"))
        .map_err(|err| format!("cannot write {}: {err}", path.display()))?;
    Ok(path.display().to_string())
}

fn first_active_step_index(mission: &Mission) -> Option<usize> {
    mission
        .steps
        .iter()
        .position(|step| !matches!(step.status.as_str(), "done" | "failed" | "needs_human"))
}

fn status_mix(mission: &Mission) -> serde_json::Map<String, Value> {
    let mut counts = std::collections::BTreeMap::<String, u64>::new();
    for step in &mission.steps {
        *counts.entry(step.status.clone()).or_insert(0) += 1;
    }
    counts
        .into_iter()
        .map(|(key, value)| (key, json!(value)))
        .collect()
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
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '/' | '.' | ':' | '@'))
    {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn first_nonempty_line(stderr: &str, stdout: &str) -> String {
    stderr
        .lines()
        .chain(stdout.lines())
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("no details")
        .to_string()
}

pub fn summarize_mission_command(mission_id: &str) -> Result<Value, String> {
    let mut mission = load_mission(mission_id)?;
    summarize_mission(&mut mission)
}
