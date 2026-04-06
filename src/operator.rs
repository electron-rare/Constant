use serde_json::{Value, json};

use crate::buddy::buddy_ask;
use crate::capabilities::{
    AGENTS, MINIMAL_STACK, SKILLS, WORKFLOW_STACK, match_skill, resolve_skill_and_agent,
    skill_by_id,
};
use crate::config::{load_fleet_config, load_models_config};
use crate::memory::{instruction_skill_sources, search_memory};
use crate::state::Mission;

pub const CHAT_ROLES: [&str; 4] = ["claude", "codex", "copilot", "vibe"];

pub fn health_value() -> Result<Value, String> {
    let models = load_models_config()?;
    let model_rows = [
        ("planner", &models.planner.model_id),
        ("buddy", &models.buddy.model_id),
        ("verify", &models.verify.model_id),
    ]
    .into_iter()
    .map(|(role, model_id)| {
        (
            role.to_string(),
            json!({
                "role": role,
                "model_id": model_id,
                "available": false,
                "loaded": false,
                "backend": "heuristic",
                "cached": false,
                "cache_path": "",
            }),
        )
    })
    .collect::<serde_json::Map<String, Value>>();

    Ok(json!({
        "mlx_python": false,
        "mlx_probe": {
            "requested": false,
            "package_present": false,
            "available": false,
            "reason": "disabled in Rust runtime",
        },
        "models": Value::Object(model_rows),
        "local_model_warmup": {
            "enabled": false,
            "download_required": false,
            "missing_model_count": 0,
            "missing_models": [],
            "startup_message": "",
        },
        "fallback_mode": models.fallback_mode,
        "agents": AGENTS,
        "skills": SKILLS,
        "recommended_skill_stack": recommended_skill_stack_value(),
    }))
}

pub fn models_status_value() -> Result<Value, String> {
    Ok(json!({
        "config": load_models_config()?,
        "runtime": {
            "backend": "heuristic",
            "python": false,
            "service": false,
        },
        "health": health_value()?,
    }))
}

pub fn chat(
    message: &str,
    mission: Option<&Mission>,
    workspace: &str,
    selected_machine: Option<&str>,
    selected_role: Option<&str>,
    _chat_history: &[Value],
) -> Result<Value, String> {
    let (prompt, explicit_skill) = extract_explicit_skill(message);
    let prompt = prompt.trim().to_string();
    let prompt_l = prompt.to_lowercase();

    let memory_payload = search_memory(&prompt, Some(workspace), Some(4))?;
    let memory_hits = memory_payload
        .get("hits")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let skill_sources = instruction_skill_sources(workspace, Some(&prompt), 4)?
        .as_array()
        .cloned()
        .unwrap_or_default();
    let memory_lines = memory_hits
        .iter()
        .take(3)
        .map(|hit| {
            format!(
                "{} {} :: {}",
                hit.get("kind").and_then(Value::as_str).unwrap_or("memory"),
                hit.get("path").and_then(Value::as_str).unwrap_or("-"),
                hit.get("snippet").and_then(Value::as_str).unwrap_or("")
            )
        })
        .collect::<Vec<_>>();

    let target_machine = match_machine_label(&prompt)?
        .or_else(|| selected_machine.map(ToString::to_string))
        .unwrap_or_else(|| fleet_labels().local);
    let target_role = match_role(&prompt)
        .or_else(|| selected_role.map(ToString::to_string))
        .unwrap_or_else(|| "codex".to_string());

    let matched_skill = explicit_skill.or_else(|| {
        if prompt.is_empty() {
            None
        } else {
            Some(match_skill(&prompt, false))
        }
    });

    let mut intent = "plain_chat".to_string();
    let mut cockpit_action = Value::Null;
    let mut buddy_note = Value::Null;
    let mut reply: String;
    let mut mission_goal = Value::Null;
    let mut routing_overrides = serde_json::Map::new();

    if contains_any(&prompt_l, &["open cockpit", "attach cockpit", "show cockpit"]) {
        intent = "cockpit_open".to_string();
        cockpit_action = json!({ "type": "open" });
        reply = "I can hand off to the full cockpit now.".to_string();
    } else if contains_any(&prompt_l, &["restart", "relance", "respawn"]) {
        intent = "cockpit_restart".to_string();
        cockpit_action = json!({
            "type": "restart",
            "machine": target_machine,
            "pane": target_role,
        });
        reply = format!("I'll restart {target_machine}:{target_role}.");
    } else if contains_any(&prompt_l, &["capture", "log", "logs", "show pane", "see pane"]) {
        intent = "cockpit_capture".to_string();
        cockpit_action = json!({
            "type": "capture",
            "machine": target_machine,
            "pane": target_role,
        });
        reply = format!("I'll capture {target_machine}:{target_role}.");
    } else if contains_any(&prompt_l, &["focus", "jump", "go to", "ouvre", "open machine"]) {
        intent = "cockpit_focus".to_string();
        cockpit_action = json!({
            "type": "focus",
            "machine": target_machine,
            "pane": target_role,
        });
        reply = format!("I'll focus {target_machine}:{target_role}.");
    } else if contains_any(
        &prompt_l,
        &[
            "memory",
            "remember",
            "decision",
            "persona",
            "what do we know",
            "qu'est-ce qu",
            "souviens",
        ],
    ) {
        intent = "memory_lookup".to_string();
        reply = if memory_lines.is_empty() {
            "No strong memory hits for that query yet.".to_string()
        } else {
            format!("Memory echoes:\n- {}", memory_lines.join("\n- "))
        };
    } else if explicit_skill.is_none()
        && prompt.ends_with('?')
        && !contains_any(
            &prompt_l,
            &[
                "fix",
                "build",
                "implement",
                "write",
                "create",
                "deploy",
                "restart",
                "capture",
                "focus",
            ],
        )
    {
        let title = mission
            .map(|entry| entry.title.as_str())
            .unwrap_or("global cockpit");
        reply = format!(
            "Constant view for {title}. selected={}:{}",
            selected_machine.unwrap_or("-"),
            selected_role.unwrap_or("-")
        );
        if !memory_lines.is_empty() {
            reply.push_str(&format!("\nMemory echoes:\n- {}", memory_lines[..memory_lines.len().min(2)].join("\n- ")));
        }
    } else {
        intent = "mission_create".to_string();
        if let Some(skill) = matched_skill {
            routing_overrides.insert("skill".to_string(), json!(skill.id));
            routing_overrides.insert("agent".to_string(), json!(skill.preferred_agent));
            routing_overrides.insert("cli".to_string(), json!(skill.preferred_cli));
        }
        let preview = heuristic_plan_preview(&prompt, workspace, &routing_overrides)?;
        let step = preview
            .get("steps")
            .and_then(Value::as_array)
            .and_then(|steps| steps.first())
            .cloned()
            .unwrap_or_else(|| json!({}));
        buddy_note = json!({
            "answer": format!(
                "Route preview agrees with {}/{}/{}.",
                step.get("machine").and_then(Value::as_str).unwrap_or("-"),
                step.get("cli").and_then(Value::as_str).unwrap_or("-"),
                step.get("backend").and_then(Value::as_str).unwrap_or("-"),
            ),
            "mode": "rust-heuristic",
        });
        reply = format!(
            "I turned that into a mission. Route preview: {}/{}/{} skill={} agent={}.",
            step.get("machine").and_then(Value::as_str).unwrap_or("-"),
            step.get("cli").and_then(Value::as_str).unwrap_or("-"),
            step.get("backend").and_then(Value::as_str).unwrap_or("-"),
            step.get("skill").and_then(Value::as_str).unwrap_or("-"),
            step.get("agent").and_then(Value::as_str).unwrap_or("-"),
        );
        mission_goal = json!(prompt);
    }

    if matches!(intent.as_str(), "plain_chat" | "memory_lookup") && buddy_note.is_null() {
        if contains_any(
            &prompt_l,
            &["route", "reroute", "machine", "cli", "pane", "codex", "claude", "vibe", "copilot"],
        ) {
            buddy_note = buddy_ask(mission, &prompt, Some(workspace))?;
        }
    }

    Ok(json!({
        "intent": intent,
        "reply": reply,
        "message": prompt,
        "mode": "rust-heuristic",
        "cockpit_action": cockpit_action,
        "buddy_note": buddy_note,
        "memory_hits": memory_hits,
        "skill_sources": skill_sources,
        "workspace": workspace,
        "mission_goal": mission_goal,
        "skill": matched_skill.map(skill_value).unwrap_or(Value::Null),
        "routing_overrides": Value::Object(routing_overrides),
    }))
}

pub fn recommended_skill_stack_value() -> Value {
    json!({
        "minimal": MINIMAL_STACK,
        "workflow": WORKFLOW_STACK,
        "layers": {
            "reflection": ["spec-planner", "architecture-brainstorm"],
            "execution": ["repo-onboarding", "task-decomposer", "pr-review-prep"],
        }
    })
}

fn skill_value(skill: &crate::capabilities::Skill) -> Value {
    json!({
        "id": skill.id,
        "label": skill.label,
        "layer": skill.layer,
        "visibility": skill.visibility,
        "summary": skill.summary,
        "preferred_cli": skill.preferred_cli,
        "preferred_agent": skill.preferred_agent,
    })
}

fn extract_explicit_skill(message: &str) -> (String, Option<&'static crate::capabilities::Skill>) {
    let raw = message.trim();
    let lowered = raw.to_lowercase();

    if let Some(payload) = lowered.strip_prefix("skill:") {
        let offset = raw.len() - payload.len();
        if let Some((skill, remainder)) = parse_skill_payload(&raw[offset..]) {
            return (remainder, Some(skill));
        }
    }
    if raw.starts_with("/skill ") {
        if let Some((skill, remainder)) = parse_skill_payload(&raw["/skill ".len()..]) {
            return (remainder, Some(skill));
        }
    }
    if raw.starts_with('/') {
        let payload = &raw[1..];
        let mut parts = payload.splitn(2, char::is_whitespace);
        if let Some(skill_id) = parts.next() {
            if let Some(skill) = skill_by_id(skill_id, false) {
                let remainder = parts.next().unwrap_or_default().trim().to_string();
                return (if remainder.is_empty() { raw.to_string() } else { remainder }, Some(skill));
            }
        }
    }
    for skill in SKILLS.iter().filter(|skill| skill.visibility == "public") {
        if lowered.contains(skill.id) {
            return (raw.to_string(), Some(skill));
        }
    }
    (raw.to_string(), None)
}

fn parse_skill_payload(payload: &str) -> Option<(&'static crate::capabilities::Skill, String)> {
    let trimmed = payload.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut parts = trimmed.splitn(2, char::is_whitespace);
    let skill_id = parts.next()?;
    let skill = skill_by_id(skill_id, false)?;
    let remainder = parts.next().unwrap_or_default().trim().to_string();
    Some((
        skill,
        if remainder.is_empty() {
            trimmed.to_string()
        } else {
            remainder
        },
    ))
}

fn heuristic_plan_preview(
    goal: &str,
    _workspace: &str,
    overrides: &serde_json::Map<String, Value>,
) -> Result<Value, String> {
    let resolution = resolve_skill_and_agent(
        Some(goal),
        overrides.get("skill").and_then(Value::as_str),
        overrides.get("agent").and_then(Value::as_str),
        overrides.get("cli").and_then(Value::as_str),
    );
    let skill = resolution.skill;
    let cli = resolution.cli;
    let machine = overrides
        .get("machine")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .unwrap_or_else(|| route_machine(goal, skill.id));
    let backend = overrides
        .get("backend")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .unwrap_or_else(|| route_backend(&machine, cli, goal));
    Ok(json!({
        "title": goal.lines().next().unwrap_or_default().trim().chars().take(80).collect::<String>(),
        "summary": format!("Route the mission to {machine} using {cli} via {backend} for skill {}.", skill.id),
        "steps": [{
            "step_id": "step-1",
            "kind": "task",
            "title": format!("Execute mission on {machine}"),
            "prompt": goal,
            "machine": machine,
            "backend": backend,
            "cli": cli,
            "agent": resolution.agent.id,
            "agent_role": resolution.agent.role,
            "skill": skill.id,
            "skill_summary": skill.summary,
            "status": "pending",
            "attempt": 0,
            "depends_on": [],
            "artifact_refs": [],
            "result_summary": "",
        }]
    }))
}

fn match_machine_label(message: &str) -> Result<Option<String>, String> {
    let message_l = message.to_lowercase();
    let fleet = load_fleet_config()?;
    Ok(fleet
        .machines
        .iter()
        .find(|machine| message_l.contains(&machine.label.to_lowercase()))
        .map(|machine| machine.label.clone()))
}

fn match_role(message: &str) -> Option<String> {
    let message_l = message.to_lowercase();
    CHAT_ROLES
        .iter()
        .find(|role| message_l.contains(**role))
        .map(|role| (*role).to_string())
}

struct FleetLabels {
    local: String,
    builder_a: String,
    builder_b: String,
    edge_a: String,
    lab_a: String,
}

fn fleet_labels() -> FleetLabels {
    let fleet = load_fleet_config().unwrap_or_default();
    let labels = fleet
        .machines
        .iter()
        .map(|machine| machine.label.clone())
        .collect::<Vec<_>>();
    FleetLabels {
        local: fleet
            .machines
            .iter()
            .find(|machine| machine.label == fleet.local_machine)
            .map(|machine| machine.label.clone())
            .unwrap_or_else(|| labels.first().cloned().unwrap_or_else(|| "command-center".to_string())),
        builder_a: labels.get(1).cloned().unwrap_or_else(|| "builder-a".to_string()),
        builder_b: labels.get(2).cloned().unwrap_or_else(|| "builder-b".to_string()),
        edge_a: labels.get(3).cloned().unwrap_or_else(|| "edge-a".to_string()),
        lab_a: labels.get(4).cloned().unwrap_or_else(|| "lab-a".to_string()),
    }
}

fn route_machine(goal: &str, skill_id: &str) -> String {
    let labels = fleet_labels();
    let goal_l = goal.to_lowercase();
    if matches!(skill_id, "spec-planner" | "repo-onboarding" | "task-decomposer") {
        return labels.local;
    }
    if skill_id == "architecture-brainstorm" {
        return labels.lab_a;
    }
    if skill_id == "pr-review-prep" {
        return labels.builder_a;
    }
    if skill_id == "ops-deployment" {
        return labels.edge_a;
    }
    if skill_id == "debug-restoration"
        && contains_any(&goal_l, &["performance", "deep", "benchmark", "compiler", "cuda"])
    {
        return labels.builder_b;
    }
    if contains_any(&goal_l, &["ssh", "shell", "fleet", "ops", "network", "infra"]) {
        return labels.edge_a;
    }
    if contains_any(&goal_l, &["refactor", "performance", "deep", "cuda", "compiler", "benchmark"]) {
        return labels.builder_b;
    }
    if contains_any(&goal_l, &["review", "audit", "test", "qa", "docs"]) {
        return labels.builder_a;
    }
    if contains_any(&goal_l, &["experiment", "prototype", "sandbox", "branch"]) {
        return labels.lab_a;
    }
    labels.local
}

fn route_backend(machine: &str, cli: &str, goal: &str) -> String {
    let local_machine = fleet_labels().local;
    let goal_l = goal.to_lowercase();
    if machine == local_machine
        && matches!(cli, "claude" | "codex")
        && contains_any(&goal_l, &["parallel", "team", "multi-agent", "compare"])
    {
        return "omc".to_string();
    }
    if machine == local_machine {
        "cli-local".to_string()
    } else {
        "cli-ssh".to_string()
    }
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}
