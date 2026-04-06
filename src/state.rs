use std::fs::{self, OpenOptions};
use std::io::Write;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::config::ModelsConfig;
use crate::paths;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MissionStep {
    #[serde(default)]
    pub step_id: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub machine: String,
    #[serde(default)]
    pub backend: String,
    #[serde(default)]
    pub cli: String,
    #[serde(default)]
    pub agent: String,
    #[serde(default)]
    pub agent_role: Option<String>,
    #[serde(default)]
    pub skill: Option<String>,
    #[serde(default)]
    pub skill_summary: Option<String>,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub attempt: u32,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub artifact_refs: Vec<String>,
    #[serde(default)]
    pub result_summary: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mission {
    pub mission_id: String,
    pub title: String,
    pub goal: String,
    pub workspace: String,
    pub status: String,
    #[serde(default = "default_priority")]
    pub priority: String,
    pub created_at: String,
    pub updated_at: String,
    pub planner_model: String,
    pub buddy_model: String,
    pub verify_model: String,
    #[serde(default = "default_owner")]
    pub owner: String,
    #[serde(default)]
    pub routing_overrides: Map<String, Value>,
    #[serde(default)]
    pub steps: Vec<MissionStep>,
    #[serde(default)]
    pub artifacts: Vec<String>,
    #[serde(default)]
    pub meta: Map<String, Value>,
    #[serde(default)]
    pub planner_summary: Option<String>,
    #[serde(default)]
    pub buddy_review: Option<Value>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

fn default_priority() -> String {
    "normal".to_string()
}

fn default_owner() -> String {
    "Constant".to_string()
}

pub fn now_utc() -> String {
    let output = Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output();
    match output {
        Ok(result) if result.status.success() => {
            String::from_utf8_lossy(&result.stdout).trim().to_string()
        }
        _ => "1970-01-01T00:00:00Z".to_string(),
    }
}

pub fn new_mission(
    goal: &str,
    workspace: &str,
    routing_overrides: Option<Map<String, Value>>,
    models: &ModelsConfig,
) -> Mission {
    let mission_id = mission_id();
    let timestamp = now_utc();
    let title_line = goal.trim().lines().next().unwrap_or_default().trim();
    let title = if title_line.is_empty() {
        format!("mission-{mission_id}")
    } else {
        title_line.chars().take(80).collect()
    };

    let mut meta = Map::new();
    meta.insert("schema_version".to_string(), json!(1));
    meta.insert("tool_version".to_string(), json!(env!("CARGO_PKG_VERSION")));

    Mission {
        mission_id,
        title,
        goal: goal.to_string(),
        workspace: workspace.to_string(),
        status: "draft".to_string(),
        priority: default_priority(),
        created_at: timestamp.clone(),
        updated_at: timestamp,
        planner_model: models.planner.model_id.clone(),
        buddy_model: models.buddy.model_id.clone(),
        verify_model: models.verify.model_id.clone(),
        owner: default_owner(),
        routing_overrides: routing_overrides.unwrap_or_default(),
        steps: Vec::new(),
        artifacts: Vec::new(),
        meta,
        planner_summary: None,
        buddy_review: None,
        extra: Map::new(),
    }
}

pub fn mission_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or_default();
    let pid = std::process::id() as u128;
    format!("{:012x}", (nanos ^ pid) & 0xffff_ffff_ffff)
}

pub fn save_mission(mission: &mut Mission) -> Result<(), String> {
    paths::ensure_runtime_dirs()?;
    mission.updated_at = now_utc();
    let path = paths::mission_file(&mission.mission_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("cannot create {}: {err}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(mission)
        .map_err(|err| format!("cannot encode mission {}: {err}", mission.mission_id))?;
    fs::write(&path, format!("{text}\n"))
        .map_err(|err| format!("cannot write {}: {err}", path.display()))
}

pub fn load_mission(mission_id: &str) -> Result<Mission, String> {
    let path = paths::mission_file(mission_id);
    let text = fs::read_to_string(&path)
        .map_err(|err| format!("cannot read {}: {err}", path.display()))?;
    serde_json::from_str(&text).map_err(|err| format!("cannot parse {}: {err}", path.display()))
}

pub fn list_missions() -> Result<Vec<Mission>, String> {
    paths::ensure_runtime_dirs()?;
    let mut paths_list = Vec::new();
    for entry in fs::read_dir(paths::missions_dir())
        .map_err(|err| format!("cannot read {}: {err}", paths::missions_dir().display()))?
    {
        let entry = entry.map_err(|err| format!("cannot iterate missions dir: {err}"))?;
        let mission_file = entry.path().join("mission.json");
        if mission_file.exists() {
            paths_list.push(mission_file);
        }
    }
    paths_list.sort();

    let mut missions = Vec::new();
    for path in paths_list {
        let text = fs::read_to_string(&path)
            .map_err(|err| format!("cannot read {}: {err}", path.display()))?;
        let mission: Mission = serde_json::from_str(&text)
            .map_err(|err| format!("cannot parse {}: {err}", path.display()))?;
        missions.push(mission);
    }
    Ok(missions)
}

pub fn append_event(mission_id: &str, event_type: &str, payload: Value) -> Result<(), String> {
    let path = paths::mission_events_file(mission_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("cannot create {}: {err}", parent.display()))?;
    }
    let event = json!({
        "timestamp": now_utc(),
        "type": event_type,
        "payload": payload,
    });
    append_json_line(&path, &event)
}

#[allow(dead_code)]
pub fn write_artifact(mission_id: &str, name: &str, payload: &Value) -> Result<String, String> {
    let artifact_dir = paths::mission_artifacts_dir(mission_id);
    fs::create_dir_all(&artifact_dir)
        .map_err(|err| format!("cannot create {}: {err}", artifact_dir.display()))?;
    let path = artifact_dir.join(name);
    let text = serde_json::to_string_pretty(payload)
        .map_err(|err| format!("cannot encode artifact {name}: {err}"))?;
    fs::write(&path, format!("{text}\n"))
        .map_err(|err| format!("cannot write {}: {err}", path.display()))?;
    Ok(path.display().to_string())
}

pub fn mission_summary_value(mission: &Mission) -> Value {
    json!({
        "mission_id": mission.mission_id,
        "title": mission.title,
        "status": mission.status,
        "workspace": mission.workspace,
        "steps": mission.steps.iter().map(step_summary_value).collect::<Vec<_>>(),
    })
}

pub fn step_summary_value(step: &MissionStep) -> Value {
    json!({
        "step_id": step.step_id,
        "status": step.status,
        "machine": step.machine,
        "backend": step.backend,
        "cli": step.cli,
        "agent": step.agent,
        "agent_role": step.agent_role,
        "skill": step.skill,
        "skill_summary": step.skill_summary,
        "attempt": step.attempt,
    })
}

#[allow(dead_code)]
pub fn first_active_step(mission: &Mission) -> Option<&MissionStep> {
    mission
        .steps
        .iter()
        .find(|step| !matches!(step.status.as_str(), "done" | "failed" | "needs_human"))
}

#[allow(dead_code)]
pub fn first_active_step_mut(mission: &mut Mission) -> Option<&mut MissionStep> {
    mission
        .steps
        .iter_mut()
        .find(|step| !matches!(step.status.as_str(), "done" | "failed" | "needs_human"))
}

pub fn mission_events_text(mission_id: &str) -> Result<String, String> {
    let path = paths::mission_events_file(mission_id);
    fs::read_to_string(&path).map_err(|err| format!("cannot read {}: {err}", path.display()))
}

fn append_json_line(path: &std::path::Path, value: &Value) -> Result<(), String> {
    let mut handle = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|err| format!("cannot open {}: {err}", path.display()))?;
    let line =
        serde_json::to_string(value).map_err(|err| format!("cannot encode JSON line: {err}"))?;
    handle
        .write_all(format!("{line}\n").as_bytes())
        .map_err(|err| format!("cannot append {}: {err}", path.display()))
}
