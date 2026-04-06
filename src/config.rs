use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::paths;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MachineConfig {
    pub label: String,
    pub target: String,
    #[serde(default)]
    pub auto_clis: Vec<String>,
    #[serde(default)]
    pub manual_clis: Vec<String>,
    #[serde(default)]
    pub backends: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetConfig {
    pub version: u32,
    pub local_machine: String,
    pub repo_dir: String,
    pub machines: Vec<MachineConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EnableMlx {
    Bool(bool),
    String(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRoleConfig {
    pub role: String,
    pub model_id: String,
    pub max_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelsConfig {
    pub version: u32,
    pub enable_mlx: EnableMlx,
    pub planner: ModelRoleConfig,
    pub buddy: ModelRoleConfig,
    pub verify: ModelRoleConfig,
    pub fallback_mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    pub version: u32,
    pub local_store_path: String,
    pub qdrant_url: String,
    pub qdrant_collection: String,
    pub workspace_enrollments: Vec<String>,
    pub instruction_weights: BTreeMap<String, f64>,
    pub max_chunks_per_query: u32,
    pub vector_dimensions: u32,
}

impl Default for FleetConfig {
    fn default() -> Self {
        Self {
            version: 1,
            local_machine: "command-center".to_string(),
            repo_dir: "$HOME/constant".to_string(),
            machines: vec![
                MachineConfig {
                    label: "command-center".to_string(),
                    target: "local".to_string(),
                    auto_clis: vec!["codex".into(), "vibe".into(), "claude".into()],
                    manual_clis: vec!["copilot".into()],
                    backends: vec!["omc".into(), "cli-local".into(), "cockpit".into()],
                },
                MachineConfig {
                    label: "builder-a".to_string(),
                    target: "dev@builder-a".to_string(),
                    auto_clis: vec!["codex".into(), "vibe".into(), "claude".into()],
                    manual_clis: vec!["copilot".into()],
                    backends: vec!["cli-ssh".into(), "cockpit".into()],
                },
                MachineConfig {
                    label: "builder-b".to_string(),
                    target: "dev@builder-b".to_string(),
                    auto_clis: vec!["codex".into(), "vibe".into(), "claude".into()],
                    manual_clis: vec!["copilot".into()],
                    backends: vec!["cli-ssh".into(), "cockpit".into()],
                },
                MachineConfig {
                    label: "edge-a".to_string(),
                    target: "dev@edge-a".to_string(),
                    auto_clis: vec!["codex".into(), "vibe".into(), "claude".into()],
                    manual_clis: vec!["copilot".into()],
                    backends: vec!["cli-ssh".into(), "cockpit".into()],
                },
                MachineConfig {
                    label: "lab-a".to_string(),
                    target: "dev@lab-a".to_string(),
                    auto_clis: vec!["codex".into(), "vibe".into(), "claude".into()],
                    manual_clis: vec!["copilot".into()],
                    backends: vec!["cli-ssh".into(), "cockpit".into()],
                },
            ],
        }
    }
}

impl Default for ModelsConfig {
    fn default() -> Self {
        Self {
            version: 1,
            enable_mlx: EnableMlx::String("auto".to_string()),
            planner: ModelRoleConfig {
                role: "planner".to_string(),
                model_id: "mlx-community-staging/Llama-3.2-3B-Instruct-mlx-4Bit".to_string(),
                max_tokens: 900,
            },
            buddy: ModelRoleConfig {
                role: "buddy".to_string(),
                model_id: "mlx-community/Qwen2.5-Coder-3B-Instruct-4bit".to_string(),
                max_tokens: 900,
            },
            verify: ModelRoleConfig {
                role: "verify".to_string(),
                model_id: "mlx-community-staging/Llama-3.2-3B-Instruct-mlx-4Bit".to_string(),
                max_tokens: 700,
            },
            fallback_mode: "heuristic".to_string(),
        }
    }
}

impl Default for MemoryConfig {
    fn default() -> Self {
        let mut weights = BTreeMap::new();
        weights.insert("workspace".to_string(), 1.0);
        weights.insert("repo".to_string(), 0.85);
        weights.insert("ancestor".to_string(), 0.65);
        weights.insert("user".to_string(), 0.45);
        weights.insert("default".to_string(), 0.2);

        Self {
            version: 1,
            local_store_path: paths::memory_store_path().display().to_string(),
            qdrant_url: String::new(),
            qdrant_collection: "constant_memory".to_string(),
            workspace_enrollments: Vec::new(),
            instruction_weights: weights,
            max_chunks_per_query: 8,
            vector_dimensions: 96,
        }
    }
}

pub fn load_fleet_config() -> Result<FleetConfig, String> {
    let mut config = load_with_legacy(
        &paths::fleet_toml_path(),
        &paths::fleet_json_path(),
        &paths::fleet_yaml_path(),
        FleetConfig::default(),
    )?;
    normalize_fleet(&mut config);
    save_with_mirror(
        &paths::fleet_toml_path(),
        &paths::fleet_json_path(),
        &config,
    )?;
    Ok(config)
}

pub fn load_models_config() -> Result<ModelsConfig, String> {
    let config = load_with_legacy(
        &paths::models_toml_path(),
        &paths::models_json_path(),
        &paths::models_yaml_path(),
        ModelsConfig::default(),
    )?;
    save_with_mirror(
        &paths::models_toml_path(),
        &paths::models_json_path(),
        &config,
    )?;
    Ok(config)
}

pub fn load_memory_config() -> Result<MemoryConfig, String> {
    let config = load_with_legacy(
        &paths::memory_toml_path(),
        &paths::memory_json_path(),
        &paths::memory_yaml_path(),
        MemoryConfig::default(),
    )?;
    save_with_mirror(
        &paths::memory_toml_path(),
        &paths::memory_json_path(),
        &config,
    )?;
    Ok(config)
}

#[allow(dead_code)]
pub fn save_memory_config(config: &MemoryConfig) -> Result<(), String> {
    save_with_mirror(
        &paths::memory_toml_path(),
        &paths::memory_json_path(),
        config,
    )
}

pub fn fleet_machine<'a>(
    fleet: &'a FleetConfig,
    needle: &str,
) -> Result<&'a MachineConfig, String> {
    fleet
        .machines
        .iter()
        .find(|machine| machine.label == needle || machine.target == needle)
        .ok_or_else(|| format!("Unknown machine: {needle}"))
}

fn normalize_fleet(config: &mut FleetConfig) {
    for machine in &mut config.machines {
        machine.backends = machine
            .backends
            .iter()
            .map(|backend| {
                if backend == "zellij" {
                    "cockpit".to_string()
                } else {
                    backend.clone()
                }
            })
            .collect();
    }
}

fn load_with_legacy<T>(
    toml_path: &Path,
    json_path: &Path,
    legacy_yaml_path: &Path,
    default: T,
) -> Result<T, String>
where
    T: Serialize + DeserializeOwned + Clone,
{
    paths::ensure_runtime_dirs()?;

    if toml_path.exists() {
        return parse_toml_file(toml_path);
    }
    if json_path.exists() {
        return parse_json_file(json_path);
    }
    if legacy_yaml_path.exists() {
        return parse_json_file(legacy_yaml_path);
    }

    save_with_mirror(toml_path, json_path, &default)?;
    Ok(default)
}

fn parse_toml_file<T>(path: &Path) -> Result<T, String>
where
    T: DeserializeOwned,
{
    let text =
        fs::read_to_string(path).map_err(|err| format!("cannot read {}: {err}", path.display()))?;
    toml::from_str(&text).map_err(|err| format!("cannot parse {}: {err}", path.display()))
}

fn parse_json_file<T>(path: &Path) -> Result<T, String>
where
    T: DeserializeOwned,
{
    let text =
        fs::read_to_string(path).map_err(|err| format!("cannot read {}: {err}", path.display()))?;
    serde_json::from_str(&text).map_err(|err| format!("cannot parse {}: {err}", path.display()))
}

fn save_with_mirror<T>(toml_path: &Path, json_path: &Path, payload: &T) -> Result<(), String>
where
    T: Serialize,
{
    let toml_text =
        toml::to_string_pretty(payload).map_err(|err| format!("cannot encode TOML: {err}"))?;
    let json_text = serde_json::to_string_pretty(payload)
        .map_err(|err| format!("cannot encode JSON: {err}"))?;

    if let Some(parent) = toml_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("cannot create {}: {err}", parent.display()))?;
    }
    if let Some(parent) = json_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("cannot create {}: {err}", parent.display()))?;
    }

    fs::write(toml_path, format!("{toml_text}\n"))
        .map_err(|err| format!("cannot write {}: {err}", toml_path.display()))?;
    fs::write(json_path, format!("{json_text}\n"))
        .map_err(|err| format!("cannot write {}: {err}", json_path.display()))?;
    Ok(())
}
