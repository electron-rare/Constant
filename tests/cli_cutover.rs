use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

fn unique_temp_dir(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("constant-cli-test-{label}-{unique}"));
    fs::create_dir_all(&path).expect("create temp dir");
    path
}

fn constant_output(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_constant"))
        .args(args)
        .env("HOME", home)
        .output()
        .expect("run constant")
}

fn write_json(path: &Path, payload: Value) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dir");
    }
    fs::write(path, format!("{}\n", serde_json::to_string_pretty(&payload).unwrap()))
        .expect("write json");
}

fn write_mission(home: &Path, mission_id: &str, status: &str, workspace: &Path) {
    let path = home
        .join(".cache/constant/missions")
        .join(mission_id)
        .join("mission.json");
    write_json(
        &path,
        json!({
            "mission_id": mission_id,
            "title": format!("Mission {mission_id}"),
            "goal": "Ship the migration",
            "workspace": workspace.display().to_string(),
            "status": status,
            "priority": "normal",
            "created_at": "2026-04-06T08:00:00Z",
            "updated_at": "2026-04-06T08:00:00Z",
            "planner_model": "heuristic",
            "buddy_model": "heuristic",
            "verify_model": "heuristic",
            "owner": "Constant",
            "routing_overrides": {},
            "steps": [],
            "artifacts": [],
            "meta": {},
        }),
    );
}

fn stable_hash_hex(value: &str) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in value.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn workspace_slug(workspace: &Path) -> String {
    let resolved = workspace
        .canonicalize()
        .unwrap_or_else(|_| workspace.to_path_buf());
    let name = resolved
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("workspace");
    format!("{name}-{}", &stable_hash_hex(&resolved.display().to_string())[..10])
}

#[test]
fn fleet_config_get_reads_existing_config() {
    let home = unique_temp_dir("fleet-config-get");
    let config_path = home.join(".config/constant/fleet.json");
    write_json(
        &config_path,
        json!({
            "version": 1,
            "local_machine": "command-center",
            "repo_dir": "$HOME/constant",
            "machines": [
                { "label": "command-center", "target": "local", "auto_clis": [], "manual_clis": [], "backends": ["cockpit"] },
                { "label": "builder-a", "target": "dev@builder-a", "auto_clis": [], "manual_clis": [], "backends": ["cli-ssh"] }
            ],
        }),
    );

    let repo_dir = constant_output(&home, &["fleet", "config-get", "repo_dir"]);
    assert!(repo_dir.status.success());
    assert_eq!(String::from_utf8_lossy(&repo_dir.stdout).trim(), "$HOME/constant");

    let machine_specs = constant_output(&home, &["fleet", "config-get", "machine_specs"]);
    assert!(machine_specs.status.success());
    let lines = String::from_utf8_lossy(&machine_specs.stdout);
    assert!(lines.contains("command-center=local"));
    assert!(lines.contains("builder-a=dev@builder-a"));
}

#[test]
fn fleet_render_scan_json_emits_candidate_payload() {
    let home = unique_temp_dir("fleet-render-scan-json");
    let scan_file = home.join("scan.tsv");
    fs::write(
        &scan_file,
        "builder-a\tdev\tbuilder-a\t22\tyes\tbuilder-a\tDarwin\t/Users/dev\t\n",
    )
    .unwrap();

    let output = constant_output(
        &home,
        &[
            "fleet",
            "render-scan-json",
            scan_file.to_str().expect("scan path"),
        ],
    );
    assert!(output.status.success());
    let payload: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(payload["candidates"][0]["host"], "builder-a");
    assert_eq!(payload["candidates"][0]["port"], 22);
    assert_eq!(payload["candidates"][0]["reachable"], true);
}

#[test]
fn fleet_write_config_generates_json_file() {
    let home = unique_temp_dir("fleet-write-config");
    let finalized = home.join("finalized.tsv");
    let output_path = home.join("fleet.json");
    fs::write(
        &finalized,
        "command-center\tlocal\tlocal\tlocal\nbuilder-a\tremote\tdev\tbuilder-a\n",
    )
    .unwrap();

    let output = constant_output(
        &home,
        &[
            "fleet",
            "write-config",
            finalized.to_str().expect("finalized path"),
            "--output",
            output_path.to_str().expect("output path"),
            "--repo-dir",
            "$HOME/constant",
        ],
    );
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), output_path.display().to_string());

    let payload: Value = serde_json::from_str(&fs::read_to_string(&output_path).unwrap()).unwrap();
    assert_eq!(payload["local_machine"], "command-center");
    assert_eq!(payload["machines"][1]["target"], "dev@builder-a");
}

#[test]
fn memory_sync_qdrant_skips_when_url_is_not_configured() {
    let home = unique_temp_dir("memory-sync");

    let output = constant_output(&home, &["memory", "sync-qdrant", "--json"]);
    assert!(output.status.success());
    let payload: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(payload["skipped"], true);
    assert_eq!(payload["ok"], false);
}

#[test]
fn mission_delete_blocks_active_missions() {
    let home = unique_temp_dir("mission-delete-blocked");
    let workspace = home.join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    write_mission(&home, "mission-running", "running", &workspace);

    let output = constant_output(
        &home,
        &["mission", "delete", "mission-running", "--json"],
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot be deleted yet"));
}

#[test]
fn mission_delete_removes_terminal_thread_files() {
    let home = unique_temp_dir("mission-delete-terminal");
    let workspace = home.join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    write_mission(&home, "mission-done", "done", &workspace);

    let mission_chat = home.join(".cache/constant/chat/missions/mission-done.ndjson");
    if let Some(parent) = mission_chat.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(
        &mission_chat,
        "{\"timestamp\":\"2026-04-06T08:01:00Z\",\"role\":\"constant\",\"content\":\"Done\"}\n",
    )
    .unwrap();

    let mission_index = home
        .join(".local/share/constant/indexes/chat/threads/mission-mission-done.json");
    write_json(
        &mission_index,
        json!({
            "thread_key": "mission:mission-done",
            "workspace": workspace.display().to_string(),
            "mission_id": "mission-done",
            "kind": "mission",
            "title": "Mission mission-done",
            "mission_status": "done",
            "message_count": 1,
            "last_role": "constant",
            "last_preview": "Done",
            "last_timestamp": "2026-04-06T08:01:00Z",
            "chat_mtime": 1,
            "updated_at": "2026-04-06T08:01:00Z",
        }),
    );

    let view_state = home.join(format!(
        ".local/share/constant/indexes/chat/views/workspace-{}.json",
        workspace_slug(&workspace)
    ));
    write_json(
        &view_state,
        json!({
            "seen_counts": {
                "mission:mission-done": 1
            }
        }),
    );

    let output = constant_output(&home, &["mission", "delete", "mission-done", "--json"]);
    assert!(output.status.success());
    let payload: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(payload["deleted"], true);

    assert!(!home.join(".cache/constant/missions/mission-done").exists());
    assert!(!mission_chat.exists());
    assert!(!mission_index.exists());

    let view_payload: Value = serde_json::from_str(&fs::read_to_string(&view_state).unwrap()).unwrap();
    assert!(view_payload["seen_counts"]["mission:mission-done"].is_null());
}

#[test]
fn cockpit_status_line_rebuilds_workspace_sidecar_from_chat() {
    let home = unique_temp_dir("status-line-sidecar");
    let workspace = home.join("workspace");
    fs::create_dir_all(&workspace).unwrap();

    let slug = workspace_slug(&workspace);
    let workspace_chat = home.join(format!(
        ".cache/constant/chat/workspaces/{slug}.ndjson"
    ));
    if let Some(parent) = workspace_chat.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(
        &workspace_chat,
        "{\"timestamp\":\"2026-04-06T08:00:00Z\",\"role\":\"user\",\"content\":\"hello from workspace\"}\n",
    )
    .unwrap();

    let output = constant_output(
        &home,
        &[
            "cockpit",
            "status-line",
            "--workspace",
            workspace.to_str().expect("workspace path"),
            "--scope-label",
            "fleet",
            "--max-length",
            "120",
        ],
    );
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("fleet | workspace"));
    assert!(stdout.contains("YOU: hello from workspace"));

    let sidecar = home.join(format!(
        ".local/share/constant/indexes/chat/threads/workspace-{slug}.json"
    ));
    assert!(sidecar.exists());
}
