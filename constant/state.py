from __future__ import annotations

import json
import hashlib
import uuid
from copy import deepcopy
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

from . import __version__
from .paths import (
    cache_root,
    chat_root,
    config_root,
    data_root,
    fleet_config_path,
    indexes_dir,
    memory_config_path,
    memory_sources_dir,
    missions_dir,
    models_config_path,
)


DEFAULT_FLEET: dict[str, Any] = {
    "version": 1,
    "local_machine": "command-center",
    "repo_dir": "$HOME/constant",
    "machines": [
        {
            "label": "command-center",
            "target": "local",
            "auto_clis": ["codex", "vibe", "claude"],
            "manual_clis": ["copilot"],
            "backends": ["omc", "cli-local", "cockpit"],
        },
        {
            "label": "builder-a",
            "target": "dev@builder-a",
            "auto_clis": ["codex", "vibe", "claude"],
            "manual_clis": ["copilot"],
            "backends": ["cli-ssh", "cockpit"],
        },
        {
            "label": "builder-b",
            "target": "dev@builder-b",
            "auto_clis": ["codex", "vibe", "claude"],
            "manual_clis": ["copilot"],
            "backends": ["cli-ssh", "cockpit"],
        },
        {
            "label": "edge-a",
            "target": "dev@edge-a",
            "auto_clis": ["codex", "vibe", "claude"],
            "manual_clis": ["copilot"],
            "backends": ["cli-ssh", "cockpit"],
        },
        {
            "label": "lab-a",
            "target": "dev@lab-a",
            "auto_clis": ["codex", "vibe", "claude"],
            "manual_clis": ["copilot"],
            "backends": ["cli-ssh", "cockpit"],
        },
    ],
}


DEFAULT_MODELS: dict[str, Any] = {
    "version": 1,
    "enable_mlx": "auto",
    "planner": {
        "role": "planner",
        "model_id": "mlx-community-staging/Llama-3.2-3B-Instruct-mlx-4Bit",
        "max_tokens": 900,
    },
    "buddy": {
        "role": "buddy",
        "model_id": "mlx-community/Qwen2.5-Coder-3B-Instruct-4bit",
        "max_tokens": 900,
    },
    "verify": {
        "role": "verify",
        "model_id": "mlx-community-staging/Llama-3.2-3B-Instruct-mlx-4Bit",
        "max_tokens": 700,
    },
    "fallback_mode": "heuristic",
}


DEFAULT_MEMORY: dict[str, Any] = {
    "version": 1,
    "local_store_path": str(data_root() / "memory.sqlite"),
    "qdrant_url": "",
    "qdrant_collection": "constant_memory",
    "workspace_enrollments": [],
    "instruction_weights": {
        "workspace": 1.0,
        "repo": 0.85,
        "ancestor": 0.65,
        "user": 0.45,
        "default": 0.2,
    },
    "max_chunks_per_query": 8,
    "vector_dimensions": 96,
}


def now_utc() -> str:
    return datetime.now(UTC).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def ensure_runtime_dirs() -> None:
    for path in (cache_root(), chat_root(), config_root(), data_root(), missions_dir(), indexes_dir(), memory_sources_dir()):
        path.mkdir(parents=True, exist_ok=True)


def _legacy_config_path(path: Path) -> Path:
    if path.suffix == ".json":
        return path.with_suffix(".yaml")
    return path


def _read_json_yaml(path: Path, default: dict[str, Any]) -> dict[str, Any]:
    ensure_runtime_dirs()
    legacy_path = _legacy_config_path(path)

    if not path.exists() and legacy_path != path and legacy_path.exists():
        return json.loads(legacy_path.read_text(encoding="utf-8"))

    if not path.exists():
        _write_json_yaml(path, default)
        return deepcopy(default)

    return json.loads(path.read_text(encoding="utf-8"))


def _write_json_yaml(path: Path, payload: dict[str, Any]) -> None:
    ensure_runtime_dirs()
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def load_fleet_config() -> dict[str, Any]:
    payload = _read_json_yaml(fleet_config_path(), DEFAULT_FLEET)
    for machine in payload.get("machines", []):
        backends = []
        for backend in machine.get("backends", []):
            backends.append("cockpit" if backend == "zellij" else backend)
        machine["backends"] = backends
    return payload


def load_models_config() -> dict[str, Any]:
    payload = _read_json_yaml(models_config_path(), DEFAULT_MODELS)
    merged = deepcopy(DEFAULT_MODELS)
    for key, value in payload.items():
        if isinstance(value, dict) and isinstance(merged.get(key), dict):
            merged[key].update(value)
        else:
            merged[key] = value
    return merged


def load_memory_config() -> dict[str, Any]:
    return _read_json_yaml(memory_config_path(), DEFAULT_MEMORY)


def save_memory_config(payload: dict[str, Any]) -> None:
    _write_json_yaml(memory_config_path(), payload)


def fleet_machine(label: str) -> dict[str, Any]:
    fleet = load_fleet_config()
    for machine in fleet["machines"]:
        if machine["label"] == label or machine["target"] == label:
            return machine
    raise KeyError(f"Unknown machine: {label}")


def mission_dir(mission_id: str) -> Path:
    return missions_dir() / mission_id


def mission_file(mission_id: str) -> Path:
    return mission_dir(mission_id) / "mission.json"


def mission_events_file(mission_id: str) -> Path:
    return mission_dir(mission_id) / "events.ndjson"


def mission_artifacts_dir(mission_id: str) -> Path:
    return mission_dir(mission_id) / "artifacts"


def _workspace_chat_slug(workspace: str) -> str:
    normalized = str(Path(workspace).expanduser().resolve())
    name = Path(normalized).name or "workspace"
    digest = hashlib.sha1(normalized.encode("utf-8")).hexdigest()[:10]
    return f"{name}-{digest}"


def chat_file(workspace: str, mission_id: str | None = None) -> Path:
    ensure_runtime_dirs()
    if mission_id:
        return chat_root() / "missions" / f"{mission_id}.ndjson"
    return chat_root() / "workspaces" / f"{_workspace_chat_slug(workspace)}.ndjson"


def read_chat_history(workspace: str, mission_id: str | None = None, limit: int = 80) -> list[dict[str, Any]]:
    path = chat_file(workspace, mission_id=mission_id)
    if not path.exists():
        return []

    entries: list[dict[str, Any]] = []
    for raw in path.read_text(encoding="utf-8").splitlines()[-limit:]:
        try:
            entries.append(json.loads(raw))
        except json.JSONDecodeError:
            continue
    return entries


def append_chat_message(
    role: str,
    content: str,
    *,
    workspace: str,
    mission_id: str | None = None,
    intent: str | None = None,
    machine: str | None = None,
    pane: str | None = None,
    meta: dict[str, Any] | None = None,
) -> dict[str, Any]:
    path = chat_file(workspace, mission_id=mission_id)
    path.parent.mkdir(parents=True, exist_ok=True)
    entry = {
        "timestamp": now_utc(),
        "role": role,
        "content": content,
        "intent": intent or "plain_chat",
        "workspace": str(Path(workspace).expanduser().resolve()),
        "mission_id": mission_id,
        "machine": machine,
        "pane": pane,
        "meta": meta or {},
    }
    with path.open("a", encoding="utf-8") as handle:
        handle.write(json.dumps(entry, sort_keys=True) + "\n")
    return entry


def create_mission(goal: str, workspace: str, routing_overrides: dict[str, Any] | None = None) -> dict[str, Any]:
    mission_id = uuid.uuid4().hex[:12]
    mission = {
        "mission_id": mission_id,
        "title": goal.strip().splitlines()[0][:80] or f"mission-{mission_id}",
        "goal": goal,
        "workspace": workspace,
        "status": "draft",
        "priority": "normal",
        "created_at": now_utc(),
        "updated_at": now_utc(),
        "planner_model": load_models_config()["planner"]["model_id"],
        "buddy_model": load_models_config()["buddy"]["model_id"],
        "verify_model": load_models_config()["verify"]["model_id"],
        "owner": "Constant",
        "routing_overrides": routing_overrides or {},
        "steps": [],
        "artifacts": [],
        "meta": {
            "schema_version": 1,
            "tool_version": __version__,
        },
    }
    save_mission(mission)
    append_event(mission_id, "mission.created", {"goal": goal, "workspace": workspace})
    return mission


def save_mission(mission: dict[str, Any]) -> None:
    path = mission_file(mission["mission_id"])
    path.parent.mkdir(parents=True, exist_ok=True)
    mission["updated_at"] = now_utc()
    path.write_text(json.dumps(mission, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def load_mission(mission_id: str) -> dict[str, Any]:
    return json.loads(mission_file(mission_id).read_text(encoding="utf-8"))


def list_missions() -> list[dict[str, Any]]:
    ensure_runtime_dirs()
    missions: list[dict[str, Any]] = []
    for path in sorted(missions_dir().glob("*/mission.json")):
        missions.append(json.loads(path.read_text(encoding="utf-8")))
    return missions


def append_event(mission_id: str, event_type: str, payload: dict[str, Any]) -> None:
    path = mission_events_file(mission_id)
    path.parent.mkdir(parents=True, exist_ok=True)
    event = {
        "timestamp": now_utc(),
        "type": event_type,
        "payload": payload,
    }
    with path.open("a", encoding="utf-8") as handle:
        handle.write(json.dumps(event, sort_keys=True) + "\n")


def write_artifact(mission_id: str, name: str, payload: dict[str, Any]) -> str:
    artifact_dir = mission_artifacts_dir(mission_id)
    artifact_dir.mkdir(parents=True, exist_ok=True)
    path = artifact_dir / name
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    mission = load_mission(mission_id)
    mission["artifacts"].append(str(path))
    save_mission(mission)
    return str(path)


def first_active_step(mission: dict[str, Any]) -> dict[str, Any] | None:
    for step in mission["steps"]:
        if step["status"] not in {"done", "failed", "needs_human"}:
            return step
    return None
