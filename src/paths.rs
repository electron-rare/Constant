use std::env;
use std::fs;
use std::path::PathBuf;

pub fn repo_root() -> PathBuf {
    if let Some(explicit) = env::var_os("CONSTANT_REPO_DIR") {
        return PathBuf::from(explicit);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME").map(PathBuf::from)
}

pub fn cache_root() -> PathBuf {
    home_dir()
        .map(|home| home.join(".cache/constant"))
        .unwrap_or_else(|| PathBuf::from(".cache/constant"))
}

pub fn config_root() -> PathBuf {
    home_dir()
        .map(|home| home.join(".config/constant"))
        .unwrap_or_else(|| PathBuf::from(".config/constant"))
}

pub fn data_root() -> PathBuf {
    home_dir()
        .map(|home| home.join(".local/share/constant"))
        .unwrap_or_else(|| PathBuf::from(".local/share/constant"))
}

pub fn expand_home_string(value: &str) -> PathBuf {
    if value == "~" {
        return home_dir().unwrap_or_else(|| PathBuf::from(value));
    }
    if let Some(rest) = value.strip_prefix("~/") {
        return home_dir()
            .map(|home| home.join(rest))
            .unwrap_or_else(|| PathBuf::from(value));
    }
    if value == "$HOME" {
        return home_dir().unwrap_or_else(|| PathBuf::from(value));
    }
    if let Some(rest) = value.strip_prefix("$HOME/") {
        return home_dir()
            .map(|home| home.join(rest))
            .unwrap_or_else(|| PathBuf::from(value));
    }
    PathBuf::from(value)
}

pub fn planner_dir() -> PathBuf {
    cache_root().join("planner")
}

pub fn missions_dir() -> PathBuf {
    cache_root().join("missions")
}

pub fn chat_root() -> PathBuf {
    cache_root().join("chat")
}

pub fn fleet_toml_path() -> PathBuf {
    config_root().join("fleet.toml")
}

pub fn models_toml_path() -> PathBuf {
    config_root().join("models.toml")
}

pub fn memory_toml_path() -> PathBuf {
    config_root().join("memory.toml")
}

pub fn fleet_json_path() -> PathBuf {
    config_root().join("fleet.json")
}

pub fn models_json_path() -> PathBuf {
    config_root().join("models.json")
}

pub fn memory_json_path() -> PathBuf {
    config_root().join("memory.json")
}

pub fn fleet_yaml_path() -> PathBuf {
    config_root().join("fleet.yaml")
}

pub fn models_yaml_path() -> PathBuf {
    config_root().join("models.yaml")
}

pub fn memory_yaml_path() -> PathBuf {
    config_root().join("memory.yaml")
}

pub fn memory_store_path() -> PathBuf {
    data_root().join("memory.sqlite")
}

#[allow(dead_code)]
pub fn persona_path() -> PathBuf {
    data_root().join("persona.md")
}

pub fn indexes_dir() -> PathBuf {
    data_root().join("indexes")
}

pub fn chat_indexes_dir() -> PathBuf {
    indexes_dir().join("chat")
}

pub fn chat_threads_index_dir() -> PathBuf {
    chat_indexes_dir().join("threads")
}

pub fn chat_views_dir() -> PathBuf {
    chat_indexes_dir().join("views")
}

pub fn memory_sources_dir() -> PathBuf {
    data_root().join("sources")
}

pub fn mission_dir(mission_id: &str) -> PathBuf {
    missions_dir().join(mission_id)
}

pub fn mission_file(mission_id: &str) -> PathBuf {
    mission_dir(mission_id).join("mission.json")
}

pub fn mission_events_file(mission_id: &str) -> PathBuf {
    mission_dir(mission_id).join("events.ndjson")
}

#[allow(dead_code)]
pub fn mission_artifacts_dir(mission_id: &str) -> PathBuf {
    mission_dir(mission_id).join("artifacts")
}

pub fn ensure_runtime_dirs() -> Result<(), String> {
    for path in [
        cache_root(),
        chat_root(),
        config_root(),
        data_root(),
        missions_dir(),
        indexes_dir(),
        chat_indexes_dir(),
        chat_threads_index_dir(),
        chat_views_dir(),
        memory_sources_dir(),
        planner_dir(),
    ] {
        fs::create_dir_all(&path)
            .map_err(|err| format!("cannot create {}: {err}", path.display()))?;
    }
    Ok(())
}
