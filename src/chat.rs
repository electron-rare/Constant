use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

use crate::paths;
use crate::state::{list_missions, load_mission, now_utc};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatEntry {
    pub timestamp: String,
    pub role: String,
    pub content: String,
    pub intent: String,
    pub workspace: String,
    pub mission_id: Option<String>,
    pub machine: Option<String>,
    pub pane: Option<String>,
    #[serde(default)]
    pub meta: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ThreadIndex {
    pub thread_key: String,
    pub workspace: String,
    pub mission_id: Option<String>,
    pub kind: String,
    pub title: String,
    pub mission_status: Option<String>,
    pub message_count: u64,
    pub last_role: String,
    pub last_preview: String,
    pub last_timestamp: String,
    pub chat_mtime: u64,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ThreadViewState {
    #[serde(default)]
    pub seen_counts: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ThreadSummary {
    pub thread_key: String,
    pub workspace: String,
    pub mission_id: Option<String>,
    pub kind: String,
    pub title: String,
    pub mission_status: Option<String>,
    pub message_count: u64,
    pub unread_count: u64,
    pub last_role: String,
    pub last_preview: String,
    pub last_timestamp: String,
}

pub fn resolve_workspace(workspace: &str) -> Result<String, String> {
    let expanded = paths::expand_home_string(workspace);
    match expanded.canonicalize() {
        Ok(path) => Ok(path.display().to_string()),
        Err(_) => Ok(expanded.display().to_string()),
    }
}

pub fn workspace_chat_slug(workspace: &str) -> Result<String, String> {
    let resolved = resolve_workspace(workspace)?;
    let name = Path::new(&resolved)
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("workspace");
    Ok(format!("{name}-{}", stable_hash_hex(&resolved)[..10].to_string()))
}

pub fn thread_key(workspace: &str, mission_id: Option<&str>) -> Result<String, String> {
    if let Some(mission_id) = mission_id {
        return Ok(format!("mission:{mission_id}"));
    }
    Ok(format!("workspace:{}", resolve_workspace(workspace)?))
}

pub fn chat_file(workspace: &str, mission_id: Option<&str>) -> Result<PathBuf, String> {
    paths::ensure_runtime_dirs()?;
    if let Some(mission_id) = mission_id {
        return Ok(paths::chat_root().join("missions").join(format!("{mission_id}.ndjson")));
    }
    Ok(paths::chat_root().join("workspaces").join(format!(
        "{}.ndjson",
        workspace_chat_slug(workspace)?
    )))
}

pub fn append_chat_message(
    role: &str,
    content: &str,
    workspace: &str,
    mission_id: Option<&str>,
    intent: Option<&str>,
    machine: Option<&str>,
    pane: Option<&str>,
    meta: Option<serde_json::Map<String, serde_json::Value>>,
) -> Result<ChatEntry, String> {
    let resolved_workspace = resolve_workspace(workspace)?;
    let path = chat_file(&resolved_workspace, mission_id)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("cannot create {}: {err}", parent.display()))?;
    }

    let entry = ChatEntry {
        timestamp: now_utc(),
        role: role.to_string(),
        content: content.to_string(),
        intent: intent.unwrap_or("plain_chat").to_string(),
        workspace: resolved_workspace.clone(),
        mission_id: mission_id.map(ToString::to_string),
        machine: machine.map(ToString::to_string),
        pane: pane.map(ToString::to_string),
        meta: meta.unwrap_or_default(),
    };

    let line = serde_json::to_string(&entry).map_err(|err| format!("cannot encode chat entry: {err}"))?;
    let mut handle = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|err| format!("cannot open {}: {err}", path.display()))?;
    handle
        .write_all(format!("{line}\n").as_bytes())
        .map_err(|err| format!("cannot append {}: {err}", path.display()))?;

    let mission = mission_id.map(load_mission).transpose()?;
    let thread_index = refresh_thread_index(
        &resolved_workspace,
        mission_id,
        mission
            .as_ref()
            .map(|value| value.title.as_str())
            .unwrap_or("Workspace chat"),
        mission.as_ref().map(|value| value.status.as_str()),
    )?;
    if thread_index.is_none() {
        return Err(format!("failed to update thread index for {}", path.display()));
    }

    Ok(entry)
}

pub fn read_chat_history(
    workspace: &str,
    mission_id: Option<&str>,
    limit: usize,
) -> Result<Vec<ChatEntry>, String> {
    let path = chat_file(workspace, mission_id)?;
    if !path.exists() {
        return Ok(Vec::new());
    }

    let raw_lines = tail_lines(&path, limit, 256 * 1024)?;
    let mut entries = Vec::new();
    for raw in raw_lines {
        if raw.trim().is_empty() {
            continue;
        }
        if let Ok(entry) = serde_json::from_str::<ChatEntry>(&raw) {
            entries.push(entry);
        }
    }
    Ok(entries)
}

pub fn refresh_thread_index(
    workspace: &str,
    mission_id: Option<&str>,
    title: &str,
    mission_status: Option<&str>,
) -> Result<Option<ThreadIndex>, String> {
    let resolved_workspace = resolve_workspace(workspace)?;
    let path = chat_file(&resolved_workspace, mission_id)?;
    if !path.exists() {
        remove_thread_index(&resolved_workspace, mission_id)?;
        return Ok(None);
    }

    let mtime = file_mtime_secs(&path)?;
    if let Some(index) = load_thread_index(&resolved_workspace, mission_id)? {
        if index.chat_mtime >= mtime {
            let mut refreshed = index;
            refreshed.title = title.to_string();
            refreshed.mission_status = mission_status.map(ToString::to_string);
            save_thread_index(&refreshed)?;
            return Ok(Some(refreshed));
        }
    }

    let mut line_count = 0_u64;
    let mut last_entry: Option<ChatEntry> = None;
    let file = File::open(&path).map_err(|err| format!("cannot open {}: {err}", path.display()))?;
    for raw in BufReader::new(file).lines() {
        let raw = raw.map_err(|err| format!("cannot read {}: {err}", path.display()))?;
        if raw.trim().is_empty() {
            continue;
        }
        line_count += 1;
        if let Ok(entry) = serde_json::from_str::<ChatEntry>(&raw) {
            last_entry = Some(entry);
        }
    }

    let Some(last_entry) = last_entry else {
        return Ok(None);
    };
    let index = ThreadIndex {
        thread_key: thread_key(&resolved_workspace, mission_id)?,
        workspace: resolved_workspace.clone(),
        mission_id: mission_id.map(ToString::to_string),
        kind: if mission_id.is_some() {
            "mission".to_string()
        } else {
            "workspace".to_string()
        },
        title: title.to_string(),
        mission_status: mission_status.map(ToString::to_string),
        message_count: line_count,
        last_role: last_entry.role.clone(),
        last_preview: content_preview(&last_entry.content, 96),
        last_timestamp: last_entry.timestamp.clone(),
        chat_mtime: mtime,
        updated_at: now_utc(),
    };
    save_thread_index(&index)?;
    Ok(Some(index))
}

pub fn list_thread_summaries(workspace: &str) -> Result<Vec<ThreadSummary>, String> {
    let resolved_workspace = resolve_workspace(workspace)?;
    let view_state = load_thread_view_state(&resolved_workspace)?;
    let workspace_index = refresh_thread_index(&resolved_workspace, None, "Workspace chat", None)?;
    let workspace_summary = workspace_index.map(|index| to_summary(index, &view_state));

    let mut missions = list_missions()?
        .into_iter()
        .filter(|mission| mission.workspace == resolved_workspace)
        .collect::<Vec<_>>();
    missions.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));

    let mut summaries = Vec::new();
    if let Some(summary) = workspace_summary {
        summaries.push(summary);
    } else {
        summaries.push(ThreadSummary {
            thread_key: thread_key(&resolved_workspace, None)?,
            workspace: resolved_workspace.clone(),
            mission_id: None,
            kind: "workspace".to_string(),
            title: "Workspace chat".to_string(),
            mission_status: None,
            message_count: 0,
            unread_count: 0,
            last_role: String::new(),
            last_preview: "Start a conversation".to_string(),
            last_timestamp: String::new(),
        });
    }

    for mission in missions {
        if let Some(index) = refresh_thread_index(
            &resolved_workspace,
            Some(&mission.mission_id),
            &mission.title,
            Some(&mission.status),
        )? {
            summaries.push(to_summary(index, &view_state));
        }
    }

    Ok(summaries)
}

pub fn latest_chat_summary(workspace: &str) -> Result<Option<ThreadSummary>, String> {
    let mut summaries = list_thread_summaries(workspace)?;
    summaries.retain(|summary| summary.message_count > 0);
    summaries.sort_by(|left, right| {
        right
            .last_timestamp
            .cmp(&left.last_timestamp)
            .then_with(|| right.message_count.cmp(&left.message_count))
    });
    Ok(summaries.into_iter().next())
}

pub fn render_chat_dock_line(
    workspace: &str,
    scope_label: Option<&str>,
    machine_label: Option<&str>,
    max_length: usize,
) -> Result<String, String> {
    let workspace_name = Path::new(&resolve_workspace(workspace)?)
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("workspace")
        .to_string();
    let mut context = vec!["workspace".to_string(), workspace_name];
    if let Some(scope_label) = scope_label.filter(|value| !value.is_empty()) {
        context.insert(0, scope_label.to_string());
    } else if let Some(machine_label) = machine_label.filter(|value| !value.is_empty()) {
        context.insert(0, machine_label.to_string());
    }

    let line = if let Some(summary) = latest_chat_summary(workspace)? {
        let thread_prefix = if summary.kind == "mission" {
            "latest mission"
        } else {
            "latest chat"
        };
        format!(
            "{} | {}: {} | {}: {}",
            context.join(" | "),
            thread_prefix,
            summary.title,
            role_label(&summary.last_role),
            summary.last_preview
        )
    } else {
        format!(
            "{} | No chats yet. Open Constant to start.",
            context.join(" | ")
        )
    };
    Ok(clip_line(&line, max_length))
}

pub fn load_thread_view_state(workspace: &str) -> Result<ThreadViewState, String> {
    let path = thread_view_state_path(workspace)?;
    if !path.exists() {
        return Ok(ThreadViewState::default());
    }
    let text = fs::read_to_string(&path).map_err(|err| format!("cannot read {}: {err}", path.display()))?;
    serde_json::from_str(&text).map_err(|err| format!("cannot parse {}: {err}", path.display()))
}

pub fn mark_thread_seen(workspace: &str, thread_key: &str, seen_count: u64) -> Result<(), String> {
    let path = thread_view_state_path(workspace)?;
    let mut view_state = load_thread_view_state(workspace)?;
    view_state
        .seen_counts
        .insert(thread_key.to_string(), seen_count);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("cannot create {}: {err}", parent.display()))?;
    }
    let payload = serde_json::to_string_pretty(&view_state)
        .map_err(|err| format!("cannot encode thread view state: {err}"))?;
    fs::write(&path, format!("{payload}\n"))
        .map_err(|err| format!("cannot write {}: {err}", path.display()))
}

pub fn delete_mission_thread(mission_id: &str) -> Result<(), String> {
    paths::ensure_runtime_dirs()?;
    let mission = load_mission(mission_id)?;
    if matches!(mission.status.as_str(), "draft" | "planned" | "running") {
        return Err(format!(
            "Mission {mission_id} is {} and cannot be deleted yet.",
            mission.status
        ));
    }

    let workspace = resolve_workspace(&mission.workspace)?;
    let mission_chat = chat_file(&workspace, Some(mission_id))?;
    if mission_chat.exists() {
        fs::remove_file(&mission_chat)
            .map_err(|err| format!("cannot remove {}: {err}", mission_chat.display()))?;
    }
    let mission_dir = paths::mission_dir(mission_id);
    if mission_dir.exists() {
        fs::remove_dir_all(&mission_dir)
            .map_err(|err| format!("cannot remove {}: {err}", mission_dir.display()))?;
    }
    remove_thread_index(&workspace, Some(mission_id))?;

    let thread_key = thread_key(&workspace, Some(mission_id))?;
    let mut view_state = load_thread_view_state(&workspace)?;
    if view_state.seen_counts.remove(&thread_key).is_some() {
        let path = thread_view_state_path(&workspace)?;
        let payload = serde_json::to_string_pretty(&view_state)
            .map_err(|err| format!("cannot encode thread view state: {err}"))?;
        fs::write(&path, format!("{payload}\n"))
            .map_err(|err| format!("cannot write {}: {err}", path.display()))?;
    }

    Ok(())
}

fn to_summary(index: ThreadIndex, view_state: &ThreadViewState) -> ThreadSummary {
    let seen_count = view_state
        .seen_counts
        .get(&index.thread_key)
        .copied()
        .unwrap_or(0);
    ThreadSummary {
        unread_count: index.message_count.saturating_sub(seen_count),
        thread_key: index.thread_key,
        workspace: index.workspace,
        mission_id: index.mission_id,
        kind: index.kind,
        title: index.title,
        mission_status: index.mission_status,
        message_count: index.message_count,
        last_role: index.last_role,
        last_preview: if index.last_preview.is_empty() {
            "Start a conversation".to_string()
        } else {
            index.last_preview
        },
        last_timestamp: index.last_timestamp,
    }
}

fn thread_index_path(workspace: &str, mission_id: Option<&str>) -> Result<PathBuf, String> {
    if let Some(mission_id) = mission_id {
        return Ok(paths::chat_threads_index_dir().join(format!("mission-{mission_id}.json")));
    }
    Ok(paths::chat_threads_index_dir().join(format!(
        "workspace-{}.json",
        workspace_chat_slug(workspace)?
    )))
}

fn remove_thread_index(workspace: &str, mission_id: Option<&str>) -> Result<(), String> {
    let path = thread_index_path(workspace, mission_id)?;
    if path.exists() {
        fs::remove_file(&path).map_err(|err| format!("cannot remove {}: {err}", path.display()))?;
    }
    Ok(())
}

fn load_thread_index(workspace: &str, mission_id: Option<&str>) -> Result<Option<ThreadIndex>, String> {
    let path = thread_index_path(workspace, mission_id)?;
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(&path).map_err(|err| format!("cannot read {}: {err}", path.display()))?;
    let payload = serde_json::from_str(&text).map_err(|err| format!("cannot parse {}: {err}", path.display()))?;
    Ok(Some(payload))
}

fn save_thread_index(index: &ThreadIndex) -> Result<(), String> {
    let path = thread_index_path(&index.workspace, index.mission_id.as_deref())?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("cannot create {}: {err}", parent.display()))?;
    }
    let payload = serde_json::to_string_pretty(index)
        .map_err(|err| format!("cannot encode thread index: {err}"))?;
    fs::write(&path, format!("{payload}\n"))
        .map_err(|err| format!("cannot write {}: {err}", path.display()))
}

fn thread_view_state_path(workspace: &str) -> Result<PathBuf, String> {
    Ok(paths::chat_views_dir().join(format!(
        "workspace-{}.json",
        workspace_chat_slug(workspace)?
    )))
}

fn role_label(role: &str) -> &'static str {
    match role {
        "user" => "YOU",
        "constant" => "CONSTANT",
        "buddy" => "BUDDY",
        "system" => "SYSTEM",
        _ => "CHAT",
    }
}

fn content_preview(text: &str, limit: usize) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.len() <= limit {
        return normalized;
    }
    format!("{}...", normalized.chars().take(limit.saturating_sub(3)).collect::<String>().trim_end())
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

fn tail_lines(path: &Path, limit: usize, max_bytes: usize) -> Result<Vec<String>, String> {
    let mut file = File::open(path).map_err(|err| format!("cannot open {}: {err}", path.display()))?;
    let file_len = file
        .metadata()
        .map_err(|err| format!("cannot stat {}: {err}", path.display()))?
        .len() as usize;
    let mut chunk = 8192_usize;
    let mut consumed = 0_usize;
    let mut buffer = Vec::new();

    while consumed < file_len && consumed < max_bytes {
        let next = chunk.min(file_len - consumed).min(max_bytes - consumed);
        consumed += next;
        let offset = (file_len - consumed) as u64;
        file.seek(SeekFrom::Start(offset))
            .map_err(|err| format!("cannot seek {}: {err}", path.display()))?;
        let mut bytes = vec![0_u8; next];
        file.read_exact(&mut bytes)
            .map_err(|err| format!("cannot read {}: {err}", path.display()))?;
        bytes.extend(buffer);
        buffer = bytes;
        let newline_count = buffer.iter().filter(|byte| **byte == b'\n').count();
        if newline_count > limit {
            break;
        }
        chunk = (chunk * 2).min(max_bytes.max(1));
    }

    let text = String::from_utf8_lossy(&buffer);
    let mut lines = text.lines().map(ToString::to_string).collect::<Vec<_>>();
    if lines.len() > limit {
        lines = lines.split_off(lines.len() - limit);
    }
    Ok(lines)
}

fn file_mtime_secs(path: &Path) -> Result<u64, String> {
    let modified = path
        .metadata()
        .and_then(|meta| meta.modified())
        .map_err(|err| format!("cannot stat {}: {err}", path.display()))?;
    Ok(modified
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0))
}

fn stable_hash_hex(value: &str) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in value.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}
