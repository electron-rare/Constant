use serde_json::{Map, Value, json};

use crate::capabilities::{agent_for_cli, resolve_skill_and_agent, skill_by_id};
use crate::config::{FleetConfig, MachineConfig, fleet_machine};
use crate::state::{Mission, MissionStep};

pub fn plan_mission(mission: &mut Mission, fleet: &FleetConfig) -> Result<Value, String> {
    let overrides = &mission.routing_overrides;
    let override_skill = string_override(overrides, "skill");
    let override_agent = string_override(overrides, "agent");
    let override_cli = string_override(overrides, "cli");
    let resolution = resolve_skill_and_agent(
        Some(&mission.goal),
        override_skill.as_deref(),
        override_agent.as_deref(),
        override_cli.as_deref(),
    );

    let machine_label =
        string_override(overrides, "machine").unwrap_or_else(|| fleet.local_machine.clone());
    let machine_cfg = fleet_machine(fleet, &machine_label)?;
    let backend =
        string_override(overrides, "backend").unwrap_or_else(|| default_backend(machine_cfg));

    let step = MissionStep {
        step_id: "step-1".to_string(),
        kind: "single-shot".to_string(),
        prompt: mission.goal.clone(),
        machine: machine_cfg.label.clone(),
        backend,
        cli: resolution.cli.to_string(),
        agent: resolution.agent.id.to_string(),
        agent_role: Some(resolution.agent.role.to_string()),
        skill: Some(resolution.skill.id.to_string()),
        skill_summary: Some(resolution.skill.summary.to_string()),
        status: "pending".to_string(),
        attempt: 0,
        depends_on: Vec::new(),
        artifact_refs: Vec::new(),
        result_summary: String::new(),
        extra: Map::new(),
    };

    mission.steps = vec![step];
    mission.status = "planned".to_string();
    mission.title = planned_title(&mission.goal, &mission.title);
    mission.planner_summary = Some(format!(
        "Rust heuristic planner selected {} / {} on {} via {}.",
        resolution.skill.id, resolution.cli, machine_cfg.label, mission.steps[0].backend
    ));
    let buddy_review = json!({
        "mode": "rust-heuristic",
        "verdict": "agree",
        "confidence": "medium",
        "why": "Matched the goal against the skill registry and routed to the preferred agent/CLI stack.",
        "change": {
            "machine": mission.steps[0].machine,
            "cli": mission.steps[0].cli,
            "backend": mission.steps[0].backend,
        },
        "memory": {
            "store": false,
            "kind": "plan"
        }
    });
    mission.buddy_review = Some(buddy_review.clone());
    Ok(buddy_review)
}

pub fn retry_mission(mission: &mut Mission, step_id: Option<&str>) -> Result<Value, String> {
    let step_index = if let Some(target) = step_id {
        mission
            .steps
            .iter()
            .position(|entry| entry.step_id == target)
    } else {
        first_active_step_index(mission).or_else(|| mission.steps.len().checked_sub(1))
    };

    let Some(step_index) = step_index else {
        return Err("No step to retry.".to_string());
    };

    let step = mission
        .steps
        .get_mut(step_index)
        .ok_or_else(|| "No step to retry.".to_string())?;
    step.status = "pending".to_string();
    let step_id_value = step.step_id.clone();
    mission.status = "planned".to_string();
    Ok(json!({ "step_id": step_id_value }))
}

pub fn delegate_step(
    mission: &mut Mission,
    step_id: Option<&str>,
    machine: Option<&str>,
    backend: Option<&str>,
    cli: Option<&str>,
    agent: Option<&str>,
    skill: Option<&str>,
) -> Result<Value, String> {
    let step_index = if let Some(target) = step_id {
        mission
            .steps
            .iter()
            .position(|entry| entry.step_id == target)
    } else {
        first_active_step_index(mission)
    };
    let Some(step_index) = step_index else {
        return Err("No active step to delegate.".to_string());
    };

    let goal_fallback = mission.goal.clone();
    let step = mission
        .steps
        .get_mut(step_index)
        .ok_or_else(|| "No active step to delegate.".to_string())?;

    if let Some(machine) = machine {
        step.machine = machine.to_string();
    }
    if let Some(backend) = backend {
        step.backend = backend.to_string();
    }

    if skill.is_some() || agent.is_some() || cli.is_some() {
        let resolved_cli = if skill.is_none() {
            cli.or_else(|| (!step.cli.is_empty()).then_some(step.cli.as_str()))
        } else {
            cli
        };
        let resolved_agent = if skill.is_none() {
            agent.or_else(|| (!step.agent.is_empty()).then_some(step.agent.as_str()))
        } else {
            agent
        };
        let resolved = resolve_skill_and_agent(
            Some(if step.prompt.is_empty() {
                &goal_fallback
            } else {
                &step.prompt
            }),
            skill.or(step.skill.as_deref()),
            resolved_agent,
            resolved_cli,
        );
        step.skill = Some(resolved.skill.id.to_string());
        step.skill_summary = Some(resolved.skill.summary.to_string());
        step.agent = resolved.agent.id.to_string();
        step.agent_role = Some(resolved.agent.role.to_string());
        step.cli = resolved.cli.to_string();
    } else if !step.cli.is_empty() && step.agent.is_empty() {
        let mapped = agent_for_cli(&step.cli);
        step.agent = mapped.id.to_string();
        step.agent_role = Some(mapped.role.to_string());
    }

    if let Some(skill_id) = step.skill.clone() {
        if let Some(mapped_skill) = skill_by_id(&skill_id, true) {
            step.skill_summary = Some(mapped_skill.summary.to_string());
        }
    }

    step.status = "pending".to_string();
    let payload = json!({
        "step_id": step.step_id,
        "machine": step.machine,
        "backend": step.backend,
        "cli": step.cli,
        "agent": step.agent,
        "skill": step.skill,
    });
    mission.status = "planned".to_string();

    Ok(payload)
}

fn string_override(payload: &Map<String, Value>, key: &str) -> Option<String> {
    payload
        .get(key)
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn default_backend(machine: &MachineConfig) -> String {
    let local = matches!(
        machine.target.as_str(),
        "local" | "localhost" | "127.0.0.1" | "::1"
    );
    let preferred = if local {
        ["cli-local", "omc", "cockpit"]
    } else {
        ["cli-ssh", "cockpit", "cli-local"]
    };

    for backend in preferred {
        if machine
            .backends
            .iter()
            .any(|candidate| candidate == backend)
        {
            return backend.to_string();
        }
    }
    machine
        .backends
        .first()
        .cloned()
        .unwrap_or_else(|| if local { "cli-local" } else { "cli-ssh" }.to_string())
}

fn first_active_step_index(mission: &Mission) -> Option<usize> {
    mission
        .steps
        .iter()
        .position(|step| !matches!(step.status.as_str(), "done" | "failed" | "needs_human"))
}

fn planned_title(goal: &str, fallback: &str) -> String {
    let head = goal.trim().lines().next().unwrap_or_default().trim();
    if head.is_empty() {
        fallback.to_string()
    } else {
        head.chars().take(80).collect()
    }
}
