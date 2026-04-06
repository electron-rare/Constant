from __future__ import annotations

import importlib.util
import json
import re
import subprocess
import sys
import threading
from dataclasses import dataclass
from typing import Any

from .capabilities import (
    agent_for_cli,
    list_agents,
    list_skills,
    match_skill,
    recommended_skill_stack,
    resolve_skill_and_agent,
    skill_by_id,
    skill_catalog_brief,
)
from .memory import instruction_skill_sources, search_memory
from .state import load_fleet_config, load_models_config

CHAT_ROLES = ("claude", "codex", "copilot", "vibe")


def _fleet_labels() -> dict[str, str]:
    fleet = load_fleet_config()
    labels = [machine["label"] for machine in fleet["machines"]]
    local_machine = fleet.get("local_machine", labels[0] if labels else "command-center")
    return {
        "local": local_machine,
        "builder_a": labels[1] if len(labels) > 1 else "builder-a",
        "builder_b": labels[2] if len(labels) > 2 else "builder-b",
        "edge_a": labels[3] if len(labels) > 3 else "edge-a",
        "lab_a": labels[4] if len(labels) > 4 else "lab-a",
    }


def _strip_code_fences(text: str) -> str:
    stripped = text.strip()
    if stripped.startswith("```"):
        stripped = re.sub(r"^```[a-zA-Z0-9_-]*\n", "", stripped)
        stripped = re.sub(r"\n```$", "", stripped)
    return stripped.strip()


def _extract_json(text: str) -> dict[str, Any]:
    stripped = _strip_code_fences(text)
    try:
        return json.loads(stripped)
    except json.JSONDecodeError:
        start = stripped.find("{")
        end = stripped.rfind("}")
        if start >= 0 and end > start:
            return json.loads(stripped[start : end + 1])
        raise


def _lower(value: str) -> str:
    return value.lower()


def _public_skill_ids() -> list[str]:
    return [item["id"] for item in list_skills(include_internal=False)]


def _extract_explicit_skill(message: str) -> tuple[str, dict[str, Any] | None]:
    raw = message.strip()
    lowered = raw.lower()

    if lowered.startswith("skill:"):
        payload = raw[len("skill:") :].strip()
        if payload:
            parts = payload.split(maxsplit=1)
            try:
                skill = skill_by_id(parts[0], include_internal=False)
            except KeyError:
                skill = None
            if skill:
                remainder = parts[1].strip() if len(parts) > 1 else ""
                return remainder or raw, skill

    if lowered.startswith("/skill "):
        payload = raw[len("/skill ") :].strip()
        if payload:
            parts = payload.split(maxsplit=1)
            try:
                skill = skill_by_id(parts[0], include_internal=False)
            except KeyError:
                skill = None
            if skill:
                remainder = parts[1].strip() if len(parts) > 1 else ""
                return remainder or raw, skill

    if raw.startswith("/"):
        parts = raw[1:].split(maxsplit=1)
        if parts:
            try:
                skill = skill_by_id(parts[0], include_internal=False)
            except KeyError:
                skill = None
            if skill:
                remainder = parts[1].strip() if len(parts) > 1 else ""
                return remainder or raw, skill

    for skill_id in _public_skill_ids():
        token = skill_id.lower()
        if token in lowered:
            try:
                return raw, skill_by_id(skill_id, include_internal=False)
            except KeyError:
                continue
    return raw, None


def _route_machine(goal: str, skill_id: str | None = None) -> str:
    labels = _fleet_labels()
    goal_l = _lower(goal)
    if skill_id in {"spec-planner", "repo-onboarding", "task-decomposer"}:
        return labels["local"]
    if skill_id == "architecture-brainstorm":
        return labels["lab_a"]
    if skill_id == "pr-review-prep":
        return labels["builder_a"]
    if skill_id == "ops-deployment":
        return labels["edge_a"]
    if skill_id == "debug-restoration" and any(token in goal_l for token in ("performance", "deep", "benchmark", "compiler", "cuda")):
        return labels["builder_b"]
    if any(token in goal_l for token in ("ssh", "shell", "fleet", "ops", "network", "infra")):
        return labels["edge_a"]
    if any(token in goal_l for token in ("refactor", "performance", "deep", "cuda", "compiler", "benchmark")):
        return labels["builder_b"]
    if any(token in goal_l for token in ("review", "audit", "test", "qa", "docs")):
        return labels["builder_a"]
    if any(token in goal_l for token in ("experiment", "prototype", "sandbox", "branch")):
        return labels["lab_a"]
    return labels["local"]


def _route_cli(goal: str) -> str:
    skill = match_skill(goal)
    if skill.get("preferred_cli"):
        return str(skill["preferred_cli"])
    goal_l = _lower(goal)
    if any(token in goal_l for token in ("brainstorm", "idea", "alternative", "explore", "compare")):
        return "vibe"
    if any(token in goal_l for token in ("spec", "summary", "summarize", "review", "docs")):
        return "claude"
    return "codex"


def _route_backend(machine: str, cli: str, goal: str) -> str:
    local_machine = _fleet_labels()["local"]
    goal_l = _lower(goal)
    if machine == local_machine and cli in {"claude", "codex"} and any(token in goal_l for token in ("parallel", "team", "multi-agent", "compare")):
        return "omc"
    if machine == local_machine:
        return "cli-local"
    return "cli-ssh"


def _route_agent(cli: str) -> str:
    return agent_for_cli(cli)["id"]


def _heuristic_plan(goal: str, workspace: str, mission_id: str, overrides: dict[str, Any] | None = None) -> dict[str, Any]:
    overrides = overrides or {}
    resolved = resolve_skill_and_agent(
        goal=goal,
        skill_id=overrides.get("skill"),
        agent_id=overrides.get("agent"),
        cli=overrides.get("cli"),
    )
    skill = resolved["skill"]
    cli = str(resolved["cli"])
    agent = resolved["agent"]
    machine = str(overrides.get("machine") or _route_machine(goal, str(skill["id"])))
    backend = str(overrides.get("backend") or _route_backend(machine, cli, goal))
    skill_sources = instruction_skill_sources(workspace, query=goal, limit=4) if workspace else []
    return {
        "title": goal.strip().splitlines()[0][:80] or mission_id,
        "summary": f"Route the mission to {machine} using {cli} via {backend} for skill {skill['id']}.",
        "steps": [
            {
                "step_id": "step-1",
                "kind": "task",
                "title": f"Execute mission on {machine}",
                "prompt": goal,
                "machine": machine,
                "backend": backend,
                "cli": cli,
                "agent": agent["id"],
                "agent_role": agent["role"],
                "skill": skill["id"],
                "skill_summary": skill["summary"],
                "skill_sources": [item["path"] for item in skill_sources],
                "status": "pending",
                "attempt": 0,
                "depends_on": [],
                "llama_plan": "heuristic planner fallback",
                "qwen_review": "",
                "result_summary": "",
                "artifact_refs": [],
            }
        ],
    }


def _heuristic_buddy_review(goal: str, plan: dict[str, Any]) -> dict[str, Any]:
    step = plan["steps"][0]
    cli = step["cli"]
    machine = step["machine"]
    local_machine = _fleet_labels()["local"]
    suggestions: list[str] = []
    agrees = True

    if cli == "copilot":
        agrees = False
        suggestions.append("copilot is manual-only in v1; use codex or claude instead")
    if any(token in _lower(goal) for token in ("fix", "implement", "refactor", "bug")) and cli != "codex":
        agrees = False
        suggestions.append("technical execution is better routed to codex")
    if machine == local_machine and step["backend"] == "cli-ssh":
        agrees = False
        suggestions.append("local machine should use cli-local or omc, not cli-ssh")

    summary = "Buddy review agrees with the plan." if agrees else "; ".join(suggestions)
    return {
        "agrees": agrees,
        "summary": summary,
        "suggested_cli": "codex" if any("codex" in entry for entry in suggestions) else None,
        "suggested_backend": "cli-local" if machine == local_machine and step["backend"] == "cli-ssh" else None,
    }


def _heuristic_verify(step: dict[str, Any], execution: dict[str, Any]) -> dict[str, Any]:
    stdout = execution.get("stdout", "")
    stderr = execution.get("stderr", "")
    combined = f"{stdout}\n{stderr}".lower()
    return_code = execution.get("returncode", 1)

    if any(token in combined for token in ("not logged in", "/login", "device-auth", "device auth", "authentication required", "please login", "please log in")):
        return {
            "decision": "needs_human",
            "summary": "The CLI needs an interactive login or credentials refresh.",
            "confidence": "high",
        }

    if return_code != 0:
        if step.get("attempt", 0) <= 1:
            return {
                "decision": "retry",
                "summary": "Execution failed once; retry is reasonable.",
                "confidence": "medium",
            }
        return {
            "decision": "failed",
            "summary": "Execution failed after retry budget was exhausted.",
            "confidence": "high",
        }

    if "error" in combined and "success" not in combined:
        return {
            "decision": "needs_human",
            "summary": "Output contains an error marker despite zero exit status.",
            "confidence": "medium",
        }

    return {
        "decision": "done",
        "summary": "Execution completed successfully.",
        "confidence": "medium",
    }


def _heuristic_buddy_answer(prompt: str, mission: dict[str, Any] | None) -> dict[str, Any]:
    title = mission["title"] if mission else "mission"
    skill = match_skill(prompt)
    return {
        "answer": (
            f"Qwen buddy heuristic view for {title}: "
            f"skill={skill['id']}, focus on route correctness, CLI fit, and whether the selected agent matches the workflow stage. "
            f"Prompt: {prompt}"
        ),
        "mode": "heuristic",
    }


def _match_machine_label(message: str) -> str | None:
    message_l = _lower(message)
    for machine in load_fleet_config()["machines"]:
        label = machine["label"]
        if label.lower() in message_l:
            return label
    return None


def _match_role(message: str) -> str | None:
    message_l = _lower(message)
    for role in CHAT_ROLES:
        if role in message_l:
            return role
    return None


def _heuristic_chat(
    message: str,
    mission: dict[str, Any] | None,
    workspace: str,
    selected_machine: str | None,
    selected_role: str | None,
) -> dict[str, Any]:
    prompt, explicit_skill = _extract_explicit_skill(message)
    prompt = prompt.strip()
    prompt_l = _lower(prompt)
    memory_hits = search_memory(prompt, workspace=workspace, limit=4).get("hits", []) if workspace else []
    skill_sources = instruction_skill_sources(workspace, query=prompt, limit=4) if workspace else []
    memory_lines = [f"{hit['kind']} {hit['path']} :: {hit['snippet']}" for hit in memory_hits[:3]]
    buddy_note = None
    cockpit_action: dict[str, Any] | None = None
    intent = "plain_chat"

    target_machine = _match_machine_label(prompt) or selected_machine or _fleet_labels()["local"]
    target_role = _match_role(prompt) or selected_role or "codex"
    matched_skill = explicit_skill or (match_skill(prompt) if prompt else None)

    if any(token in prompt_l for token in ("open cockpit", "attach cockpit", "show cockpit")):
        intent = "cockpit_open"
        cockpit_action = {"type": "open"}
        reply = "I can hand off to the full cockpit now."
    elif any(token in prompt_l for token in ("restart", "relance", "respawn")):
        intent = "cockpit_restart"
        cockpit_action = {"type": "restart", "machine": target_machine, "pane": target_role}
        reply = f"I'll restart {target_machine}:{target_role}."
    elif any(token in prompt_l for token in ("capture", "log", "logs", "show pane", "see pane")):
        intent = "cockpit_capture"
        cockpit_action = {"type": "capture", "machine": target_machine, "pane": target_role}
        reply = f"I'll capture {target_machine}:{target_role}."
    elif any(token in prompt_l for token in ("focus", "jump", "go to", "ouvre", "open machine")):
        intent = "cockpit_focus"
        cockpit_action = {"type": "focus", "machine": target_machine, "pane": target_role}
        reply = f"I'll focus {target_machine}:{target_role}."
    elif any(token in prompt_l for token in ("memory", "remember", "decision", "persona", "what do we know", "qu'est-ce qu", "souviens")):
        intent = "memory_lookup"
        reply = "Memory lookup ready."
    elif explicit_skill is None and prompt.endswith("?") and not any(
        token in prompt_l for token in ("fix", "build", "implement", "write", "create", "deploy", "restart", "capture", "focus")
    ):
        intent = "plain_chat"
        reply = "Here's the operator view."
    else:
        intent = "mission_create"
        routing_overrides = {
            "skill": matched_skill["id"] if matched_skill else None,
            "agent": matched_skill["preferred_agent"] if matched_skill else None,
            "cli": matched_skill["preferred_cli"] if matched_skill else None,
        }
        preview = _heuristic_plan(prompt, workspace, "chat-preview", routing_overrides)
        review = _heuristic_buddy_review(prompt, preview)
        step = preview["steps"][0]
        buddy_note = {
            "answer": review["summary"],
            "mode": "heuristic",
        }
        matched_skill_id = preview["steps"][0]["skill"]
        reply = (
            f"I turned that into a mission. Route preview: "
            f"{step['machine']}/{step['cli']}/{step['backend']} "
            f"skill={matched_skill_id} agent={step['agent']}."
        )

    if intent != "mission_create" and any(
        token in prompt_l for token in ("route", "reroute", "which machine", "which cli", "codex", "claude", "vibe", "copilot")
    ):
        buddy_note = _heuristic_buddy_answer(prompt, mission)

    if intent == "memory_lookup":
        if memory_lines:
            reply = "Memory echoes:\n- " + "\n- ".join(memory_lines[:3])
        else:
            reply = "No strong memory hits for that query yet."
    elif intent == "plain_chat":
        title = mission["title"] if mission else "global cockpit"
        route_hint = f" selected={selected_machine or '-'}:{selected_role or '-'}"
        reply = f"Constant view for {title}.{route_hint}"
        if memory_lines:
            reply += "\nMemory echoes:\n- " + "\n- ".join(memory_lines[:2])

    return {
        "intent": intent,
        "reply": reply,
        "message": prompt,
        "mode": "heuristic",
        "cockpit_action": cockpit_action,
        "buddy_note": buddy_note,
        "memory_hits": memory_hits,
        "skill_sources": skill_sources,
        "workspace": workspace,
        "mission_goal": prompt if intent == "mission_create" else None,
        "skill": matched_skill,
        "routing_overrides": {
            "skill": matched_skill["id"] if matched_skill else None,
            "agent": matched_skill["preferred_agent"] if matched_skill else None,
            "cli": matched_skill["preferred_cli"] if matched_skill else None,
        }
        if intent == "mission_create" and matched_skill
        else {},
    }


def _budget_chat_history(chat_history: list[dict[str, Any]] | None, limit: int = 8) -> list[dict[str, Any]]:
    if not chat_history:
        return []
    important = [
        entry for entry in chat_history
        if entry.get("intent") in {"mission_create", "cockpit_error", "buddy_answer", "memory_lookup"}
    ]
    recent = list(chat_history[-limit:])
    merged: list[dict[str, Any]] = []
    seen: set[tuple[str, str]] = set()
    for entry in [*important[-3:], *recent]:
        key = (str(entry.get("timestamp", "")), str(entry.get("content", "")))
        if key in seen:
            continue
        seen.add(key)
        merged.append(entry)
    return merged[-limit:]


@dataclass
class ModelHealth:
    role: str
    model_id: str
    available: bool
    loaded: bool
    backend: str


class PlannerEngine:
    def __init__(self) -> None:
        self._models = load_models_config()
        self._loaded: dict[str, tuple[Any, Any]] = {}
        self._lock = threading.Lock()
        self._mlx_probe = self._probe_mlx()
        self._mlx_python = self._mlx_probe["available"]

    def _probe_mlx(self) -> dict[str, Any]:
        enable_setting = self._models.get("enable_mlx", "auto")
        requested = str(enable_setting).lower() not in {"0", "false", "off", "no"}
        package_present = importlib.util.find_spec("mlx_lm") is not None
        if not requested:
            return {"requested": False, "package_present": package_present, "available": False, "reason": "disabled"}
        if not package_present:
            return {"requested": True, "package_present": False, "available": False, "reason": "mlx_lm not installed"}
        probe = subprocess.run(
            [sys.executable, "-c", "import mlx.core as mx; print(mx.default_device())"],
            capture_output=True,
            text=True,
        )
        return {
            "requested": True,
            "package_present": True,
            "available": probe.returncode == 0,
            "reason": probe.stderr.strip() or ("ok" if probe.returncode == 0 else "mlx probe failed"),
            "stdout": probe.stdout.strip(),
            "returncode": probe.returncode,
        }

    def health(self) -> dict[str, Any]:
        health = {}
        for role in ("planner", "buddy", "verify"):
            spec = self._models[role]
            health[role] = ModelHealth(
                role=role,
                model_id=spec["model_id"],
                available=self._mlx_python,
                loaded=role in self._loaded,
                backend="mlx-python" if self._mlx_python else "heuristic",
            ).__dict__
        return {
            "mlx_python": self._mlx_python,
            "mlx_probe": self._mlx_probe,
            "models": health,
            "fallback_mode": self._models.get("fallback_mode", "heuristic"),
            "agents": list_agents(),
            "skills": list_skills(),
            "recommended_skill_stack": recommended_skill_stack(),
        }

    def _load_model(self, role: str) -> tuple[Any, Any]:
        if role in self._loaded:
            return self._loaded[role]

        if not self._mlx_python:
            raise RuntimeError("mlx_lm is not available")

        with self._lock:
            if role in self._loaded:
                return self._loaded[role]
            from mlx_lm import load  # type: ignore

            model_id = self._models[role]["model_id"]
            model, tokenizer = load(model_id)
            self._loaded[role] = (model, tokenizer)
            return model, tokenizer

    def _call_model_json(self, role: str, system_prompt: str, user_prompt: str) -> dict[str, Any]:
        if not self._mlx_python:
            raise RuntimeError("mlx_lm is not available")

        model, tokenizer = self._load_model(role)
        from mlx_lm import generate  # type: ignore

        messages = [
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": user_prompt},
        ]
        prompt = tokenizer.apply_chat_template(messages, tokenize=False, add_generation_prompt=True)
        text = generate(
            model,
            tokenizer,
            prompt=prompt,
            max_tokens=self._models[role]["max_tokens"],
            verbose=False,
        )
        return _extract_json(text)

    def _call_model_text(self, role: str, system_prompt: str, user_prompt: str) -> str:
        if not self._mlx_python:
            raise RuntimeError("mlx_lm is not available")

        model, tokenizer = self._load_model(role)
        from mlx_lm import generate  # type: ignore

        messages = [
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": user_prompt},
        ]
        prompt = tokenizer.apply_chat_template(messages, tokenize=False, add_generation_prompt=True)
        return generate(
            model,
            tokenizer,
            prompt=prompt,
            max_tokens=self._models[role]["max_tokens"],
            verbose=False,
        ).strip()

    def plan_mission(self, mission: dict[str, Any]) -> dict[str, Any]:
        overrides = mission.get("routing_overrides") or {}
        plan = _heuristic_plan(mission["goal"], mission["workspace"], mission["mission_id"], overrides)
        review = _heuristic_buddy_review(mission["goal"], plan)
        fleet = load_fleet_config()
        machine_labels = [machine["label"] for machine in fleet["machines"]]
        local_machine = fleet.get("local_machine", machine_labels[0] if machine_labels else "command-center")

        if self._mlx_python:
            system = (
                "You are Llama, the main orchestrator for a 5-machine coding fleet. "
                "Return strict JSON with keys: title, summary, steps. "
                "Each step must include: step_id, kind, title, prompt, machine, backend, cli, agent, depends_on."
            )
            user = json.dumps(
                {
                    "mission_id": mission["mission_id"],
                    "goal": mission["goal"],
                    "workspace": mission["workspace"],
                    "routing_overrides": overrides,
                    "machines": machine_labels,
                    "allowed_backends": ["omc", "cli-local", "cli-ssh", "cockpit"],
                    "allowed_clis": ["claude", "codex", "vibe"],
                    "skill_catalog": skill_catalog_brief(include_internal=True),
                },
                indent=2,
            )
            try:
                llama_plan = self._call_model_json("planner", system, user)
                if "steps" in llama_plan and llama_plan["steps"]:
                    plan = {
                        "title": llama_plan.get("title", plan["title"]),
                        "summary": llama_plan.get("summary", plan["summary"]),
                        "steps": [],
                    }
                    for index, step in enumerate(llama_plan["steps"], start=1):
                        resolved = resolve_skill_and_agent(
                            goal=step.get("prompt", mission["goal"]),
                            skill_id=step.get("skill") or overrides.get("skill"),
                            agent_id=step.get("agent") or overrides.get("agent"),
                            cli=step.get("cli") or overrides.get("cli"),
                        )
                        resolved_skill = resolved["skill"]
                        resolved_agent = resolved["agent"]
                        resolved_cli = str(resolved["cli"])
                        resolved_machine = step.get("machine") or overrides.get("machine") or _route_machine(step.get("prompt", mission["goal"]), resolved_skill["id"])
                        plan["steps"].append(
                            {
                                "step_id": step.get("step_id", f"step-{index}"),
                                "kind": step.get("kind", "task"),
                                "title": step.get("title", f"Step {index}"),
                                "prompt": step.get("prompt", mission["goal"]),
                                "machine": resolved_machine,
                                "backend": step.get("backend", _route_backend(resolved_machine, resolved_cli, mission["goal"])),
                                "cli": resolved_cli,
                                "agent": resolved_agent["id"],
                                "agent_role": resolved_agent.get("role"),
                                "skill": resolved_skill["id"],
                                "skill_summary": step.get("skill_summary", resolved_skill["summary"]),
                                "skill_sources": step.get("skill_sources", [item["path"] for item in instruction_skill_sources(mission["workspace"], query=step.get("prompt", mission["goal"]), limit=4)]),
                                "status": "pending",
                                "attempt": 0,
                                "depends_on": step.get("depends_on", []),
                                "llama_plan": llama_plan.get("summary", ""),
                                "qwen_review": "",
                                "result_summary": "",
                                "artifact_refs": [],
                            }
                        )
            except Exception:
                pass

            buddy_system = (
                "You are Qwen, the local technical buddy. Return strict JSON with keys: "
                "agrees, summary, suggested_cli, suggested_backend."
            )
            buddy_user = json.dumps({"goal": mission["goal"], "plan": plan}, indent=2)
            try:
                review = self._call_model_json("buddy", buddy_system, buddy_user)
            except Exception:
                pass

        for step in plan["steps"]:
            step["qwen_review"] = review.get("summary", "")
            if not review.get("agrees", True):
                if review.get("suggested_cli") and step["cli"] == "claude":
                    step["cli"] = review["suggested_cli"]
                    agent = agent_for_cli(step["cli"])
                    step["agent"] = agent["id"]
                    step["agent_role"] = agent["role"]
                if review.get("suggested_backend"):
                    step["backend"] = review["suggested_backend"]

        return {
            "plan": plan,
            "buddy_review": review,
        }

    def verify_step(self, mission: dict[str, Any], step: dict[str, Any], execution: dict[str, Any]) -> dict[str, Any]:
        result = _heuristic_verify(step, execution)

        if self._mlx_python:
            system = (
                "You are the orchestrator verifier. Return strict JSON with keys: "
                "decision, summary, confidence. Decisions: done, retry, failed, needs_human."
            )
            user = json.dumps(
                {
                    "mission_title": mission["title"],
                    "step": step,
                    "execution": {
                        "returncode": execution.get("returncode"),
                        "stdout_tail": execution.get("stdout", "")[-4000:],
                        "stderr_tail": execution.get("stderr", "")[-4000:],
                    },
                },
                indent=2,
            )
            try:
                result = self._call_model_json("verify", system, user)
            except Exception:
                pass

        return result

    def buddy_ask(self, mission: dict[str, Any] | None, prompt: str) -> dict[str, Any]:
        result = _heuristic_buddy_answer(prompt, mission)

        if self._mlx_python:
            system = "You are Qwen, the local buddy. Give a concise technical answer."
            user = json.dumps({"mission": mission, "prompt": prompt}, indent=2)
            try:
                result = {
                    "answer": self._call_model_text("buddy", system, user),
                    "mode": "mlx",
                }
            except Exception:
                pass

        return result

    def chat(
        self,
        message: str,
        mission: dict[str, Any] | None,
        workspace: str,
        selected_machine: str | None,
        selected_role: str | None,
        chat_history: list[dict[str, Any]] | None = None,
    ) -> dict[str, Any]:
        result = _heuristic_chat(message, mission, workspace, selected_machine, selected_role)

        if self._mlx_python:
            system = (
                "You are Constant, the main cockpit operator for a multi-machine coding fleet. "
                "Return strict JSON with keys: intent, reply. "
                "Valid intents: mission_create, cockpit_focus, cockpit_capture, cockpit_restart, cockpit_open, "
                "buddy_answer, memory_lookup, plain_chat."
            )
            user = json.dumps(
                {
                    "message": message,
                    "workspace": workspace,
                    "mission": mission,
                    "selected_machine": selected_machine,
                    "selected_role": selected_role,
                    "chat_history": _budget_chat_history(chat_history),
                    "memory_hits": result.get("memory_hits", [])[:4],
                    "buddy_note": result.get("buddy_note"),
                    "skills": list_skills(),
                    "skill_catalog": skill_catalog_brief(include_internal=True),
                    "recommended_skill_stack": recommended_skill_stack(),
                    "agents": list_agents(),
                },
                indent=2,
            )
            try:
                model_result = self._call_model_json("planner", system, user)
                result["intent"] = str(model_result.get("intent", result["intent"]))
                result["reply"] = str(model_result.get("reply", result["reply"]))
                result["mode"] = "mlx"
            except Exception:
                pass

        if result["intent"] in {"plain_chat", "memory_lookup"} and result.get("buddy_note") is None:
            prompt_l = _lower(message)
            if any(token in prompt_l for token in ("route", "reroute", "machine", "cli", "pane", "codex", "claude", "vibe", "copilot")):
                result["buddy_note"] = self.buddy_ask(mission, message)

        return result
