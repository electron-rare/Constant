from __future__ import annotations

from pathlib import Path


def repo_root() -> Path:
    return Path(__file__).resolve().parents[1]


def scripts_dir() -> Path:
    return repo_root() / "scripts"


def cache_root() -> Path:
    return Path.home() / ".cache" / "constant"


def config_root() -> Path:
    return Path.home() / ".config" / "constant"


def data_root() -> Path:
    return Path.home() / ".local" / "share" / "constant"


def planner_dir() -> Path:
    return cache_root() / "planner"


def missions_dir() -> Path:
    return cache_root() / "missions"


def chat_root() -> Path:
    return cache_root() / "chat"


def fleet_config_path() -> Path:
    return config_root() / "fleet.json"


def models_config_path() -> Path:
    return config_root() / "models.json"


def memory_config_path() -> Path:
    return config_root() / "memory.json"


def daemon_pid_path() -> Path:
    return planner_dir() / "daemon.pid"


def daemon_log_path() -> Path:
    return planner_dir() / "daemon.log"


def daemon_port_path() -> Path:
    return planner_dir() / "daemon.port"


def memory_store_path() -> Path:
    return data_root() / "memory.sqlite"


def persona_path() -> Path:
    return data_root() / "persona.md"


def indexes_dir() -> Path:
    return data_root() / "indexes"


def memory_sources_dir() -> Path:
    return data_root() / "sources"
