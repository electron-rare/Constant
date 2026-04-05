from __future__ import annotations

import importlib.util
import json
import re
import threading
from dataclasses import dataclass
from typing import Any

from .state import load_fleet_config, load_models_config


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


def _route_machine(goal: str) -> str:
    labels = _fleet_labels()
    goal_l = _lower(goal)
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
    return {
        "claude": "planner",
        "codex": "executor",
        "vibe": "analyst",
        "copilot": "assistant",
    }.get(cli, "executor")


def _heuristic_plan(goal: str, workspace: str, mission_id: str) -> dict[str, Any]:
    machine = _route_machine(goal)
    cli = _route_cli(goal)
    backend = _route_backend(machine, cli, goal)
    return {
        "title": goal.strip().splitlines()[0][:80] or mission_id,
        "summary": f"Route the mission to {machine} using {cli} via {backend}.",
        "steps": [
            {
                "step_id": "step-1",
                "kind": "task",
                "title": f"Execute mission on {machine}",
                "prompt": goal,
                "machine": machine,
                "backend": backend,
                "cli": cli,
                "agent": _route_agent(cli),
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
    return {
        "answer": f"Qwen buddy heuristic view for {title}: focus on route correctness, CLI fit, and whether codex should own the technical step. Prompt: {prompt}",
        "mode": "heuristic",
    }


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
        self._mlx_python = importlib.util.find_spec("mlx_lm") is not None

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
            "models": health,
            "fallback_mode": self._models.get("fallback_mode", "heuristic"),
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
        plan = _heuristic_plan(mission["goal"], mission["workspace"], mission["mission_id"])
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
                    "machines": machine_labels,
                    "allowed_backends": ["omc", "cli-local", "cli-ssh", "cockpit"],
                    "allowed_clis": ["claude", "codex", "vibe"],
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
                        plan["steps"].append(
                            {
                                "step_id": step.get("step_id", f"step-{index}"),
                                "kind": step.get("kind", "task"),
                                "title": step.get("title", f"Step {index}"),
                                "prompt": step.get("prompt", mission["goal"]),
                                "machine": step.get("machine", _route_machine(mission["goal"])),
                                "backend": step.get("backend", _route_backend(step.get("machine", local_machine), step.get("cli", "codex"), mission["goal"])),
                                "cli": step.get("cli", _route_cli(mission["goal"])),
                                "agent": step.get("agent", _route_agent(step.get("cli", "codex"))),
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
                    step["agent"] = _route_agent(step["cli"])
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
