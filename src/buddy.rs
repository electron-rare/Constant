use serde_json::{Value, json};

use crate::capabilities::match_skill;
use crate::state::Mission;

pub fn buddy_ask(
    mission: Option<&Mission>,
    prompt: &str,
    _workspace: Option<&str>,
) -> Result<Value, String> {
    let skill = match_skill(prompt, true);
    let title = mission
        .map(|entry| entry.title.as_str())
        .unwrap_or("global cockpit");
    let route_note = if prompt.to_lowercase().contains("route")
        || prompt.to_lowercase().contains("machine")
        || prompt.to_lowercase().contains("cli")
    {
        format!(
            "Treat this as a routing discussion first, with skill={} as the default lens.",
            skill.id
        )
    } else {
        format!(
            "Treat this as a {} discussion before execution.",
            skill.summary
        )
    };
    let answer = format!("Qwen buddy heuristic view for {title}: {route_note}");
    Ok(json!({
        "answer": answer,
        "mode": "rust-heuristic",
        "skill": {
            "id": skill.id,
            "summary": skill.summary,
            "preferred_cli": skill.preferred_cli,
            "preferred_agent": skill.preferred_agent,
        },
        "memory_hits": [],
    }))
}
