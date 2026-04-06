from __future__ import annotations

import curses
import json
import textwrap
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from .capabilities import list_skills
from .cockpit import ROLES, capture_pane, focus_machine, restart_pane, runtime_status
from .daemon import request as daemon_request
from .memory import list_decisions, memory_status, persona_markdown, prime_workspace_memory, rebuild_workspace_memory, summarize_mission
from .state import (
    append_chat_message,
    append_event,
    create_mission,
    list_missions,
    load_fleet_config,
    mission_events_file,
    read_chat_history,
    save_mission,
)


CHAT_LABELS = {
    "user": "YOU",
    "constant": "CONSTANT",
    "buddy": "BUDDY",
    "system": "SYSTEM",
}

HEXAPUS_FRAMES = (
    [
        "         .-====-.",
        "      .-'  .--.  `-.",
        "     /    ( oo )    \\",
        "    |      \\/\\/      |",
        "    |    .-____-.    |",
        "     \\__/_/ || \\_\\__/",
        "       /_  /||\\  _\\",
        "    .-'  \\/ || \\/  `-.",
        "   <_____/  ||  \\_____>",
        "        /___/\\___\\",
    ],
    [
        "         .-====-.",
        "      .-'  .--.  `-.",
        "     /    ( oo )    \\",
        "    |      \\/\\/      |",
        "    |    .-____-.    |",
        "   _/\\__/ / || \\ \\__/\\_",
        "  /_  _\\/  ||  \\/_  _\\",
        "    `-.   / || \\   .-'",
        "   <_____/  ||  \\_____>",
        "        /___/\\___\\",
    ],
    [
        "         .-====-.",
        "      .-'  .--.  `-.",
        "     /    ( -- )    \\",
        "    |      \\/\\/      |",
        "    |    .-____-.    |",
        "     \\__/_/ || \\_\\__/",
        "       /_  /||\\  _\\",
        "   .--'  \\/ || \\/  `--.",
        "  <_____/   ||   \\_____>",
        "        /___/\\___\\",
    ],
    [
        "         .-====-.",
        "      .-'  .--.  `-.",
        "     /    ( oo )    \\",
        "    |      \\/\\/      |",
        "    |    .-____-.    |",
        "  _/\\__/_/ || \\_\\__ /\\_",
        " /_  _/   /||\\   \\_  _\\",
        "   `-.   / || \\   .-'",
        "  <_____/  ||  \\_____>",
        "       /___/\\___\\",
    ],
    [
        "         .-====-.",
        "      .-'  .--.  `-.",
        "     /    ( xx )    \\",
        "    |      \\/\\/      |",
        "    |    .-____-.    |",
        "     \\__/__/||\\__\\__/",
        "       _\\_ /||\\ _/_",
        "   .--'   \\/ || \\/   `--.",
        "  <_____/   ||   \\_____>",
        "        /___/\\___\\",
    ],
)


@dataclass
class TuiLayout:
    height: int
    width: int
    top: int
    footer_height: int
    timeline_height: int
    main_height: int
    left_width: int
    right_width: int
    center_width: int
    conversation_height: int
    runtime_height: int


@dataclass
class TuiState:
    selected_index: int = 0
    selected_machine: int = 0
    selected_role_index: int = 1
    flash: str = ""
    flash_until: float = 0.0
    runtime_cache: dict[str, Any] | None = None
    runtime_refresh_at: float = 0.0
    input_mode: bool = True
    input_buffer: str = ""
    slash_menu_index: int = 0
    capture_state: dict[str, Any] | None = None
    pending_select_mission_id: str | None = None
    chat_focus: bool = True
    stream_text: str = ""
    stream_started_at: float = 0.0
    stream_duration: float = 0.0
    stream_mission_id: str | None = None

    @property
    def selected_role(self) -> str:
        return ROLES[self.selected_role_index]

    def set_flash(self, message: str, duration: float = 2.5) -> None:
        self.flash = message
        self.flash_until = time.time() + duration


def _error_line(prefix: str, exc: Exception) -> str:
    return f"{prefix}: {str(exc).strip() or exc.__class__.__name__}"


def _safe_memory_status(workspace: str) -> dict[str, Any]:
    try:
        return memory_status(workspace)
    except Exception as exc:  # noqa: BLE001
        return {
            "workspace": workspace,
            "counts": {},
            "error": _error_line("memory", exc),
        }


def _safe_persona_lines() -> list[str]:
    try:
        return [
            line[2:]
            for line in persona_markdown().splitlines()
            if line.startswith("- ")
        ]
    except Exception as exc:  # noqa: BLE001
        return [_error_line("persona", exc)]


def _safe_decision_lines(workspace: str) -> list[str]:
    try:
        decisions = list_decisions(workspace=workspace).get("decisions", [])[:3]
        return [f"{item['decision_id'].split(':')[-1]} {item['status']} {item['title']}" for item in decisions]
    except Exception as exc:  # noqa: BLE001
        return [_error_line("decisions", exc)]


def _safe_runtime_status(local_session: str, machine_session: str) -> dict[str, Any]:
    try:
        return runtime_status(local_session=local_session, machine_session=machine_session)
    except Exception as exc:  # noqa: BLE001
        return {
            "fleet_session_exists": False,
            "machines": [],
            "fleet_windows": [],
            "fleet_stderr": _error_line("cockpit", exc),
            "focused_machine": None,
            "focused_role": None,
        }


def _clip(text: str, width: int) -> str:
    if width <= 0:
        return ""
    if len(text) <= width:
        return text
    if width <= 3:
        return text[:width]
    return text[: width - 3] + "..."


def _safe_addstr(stdscr: Any, y: int, x: int, text: str, attr: int = 0) -> None:
    height, width = stdscr.getmaxyx()
    if y < 0 or y >= height or x >= width:
        return
    text = _clip(text, width - x)
    if not text:
        return
    try:
        stdscr.addstr(y, x, text, attr)
    except curses.error:
        pass


def _draw_box(stdscr: Any, y: int, x: int, height: int, width: int, title: str, title_attr: int = 0) -> None:
    if height < 3 or width < 4:
        return
    _safe_addstr(stdscr, y, x, "+" + ("-" * max(0, width - 2)) + "+")
    for row in range(y + 1, y + height - 1):
        _safe_addstr(stdscr, row, x, "|" + (" " * max(0, width - 2)) + "|")
    _safe_addstr(stdscr, y + height - 1, x, "+" + ("-" * max(0, width - 2)) + "+")
    _safe_addstr(stdscr, y, x + 2, f"[ {title} ]", title_attr)


def _write_wrapped(stdscr: Any, y: int, x: int, width: int, lines: list[str], attr: int = 0) -> int:
    row = y
    for line in lines:
        wrapped = textwrap.wrap(line, max(8, width)) or [""]
        for item in wrapped:
            _safe_addstr(stdscr, row, x, item, attr)
            row += 1
    return row


def _recent_events(mission_id: str | None, limit: int = 8) -> list[str]:
    if mission_id:
        path = mission_events_file(mission_id)
        if not path.exists():
            return []
        lines = path.read_text(encoding="utf-8").splitlines()[-limit:]
        events = []
        for raw in lines:
            try:
                event = json.loads(raw)
            except json.JSONDecodeError:
                continue
            event_type = event.get("type", "event")
            timestamp = str(event.get("timestamp", ""))[-9:-1]
            payload = event.get("payload", {})
            detail = payload.get("summary") or payload.get("step_id") or payload.get("machine") or payload.get("mission_id") or ""
            line = f"{timestamp} {event_type}"
            if detail:
                line += f" :: {detail}"
            events.append(line)
        return events[-limit:]

    events: list[str] = []
    for mission in sorted(list_missions(), key=lambda item: item.get("updated_at", ""), reverse=True)[:5]:
        events.extend(_recent_events(mission["mission_id"], limit=3))
    return events[-limit:]


def _normalize_buddy(review: dict[str, Any] | None) -> dict[str, str]:
    if not review:
        return {
            "verdict": "idle",
            "confidence": "--",
            "why": "No buddy review yet.",
            "change": "none",
            "memory": "idle",
        }

    if "verdict" in review:
        change = review.get("change") or {}
        target = "/".join(
            part for part in [change.get("machine"), change.get("cli"), change.get("backend")] if part
        ) or "none"
        memory = review.get("memory", {})
        memory_hint = "store" if memory.get("store") else "ignore"
        return {
            "verdict": str(review.get("verdict", "warn")),
            "confidence": str(review.get("confidence", "medium")),
            "why": str(review.get("why", review.get("summary", ""))) or "No reason provided.",
            "change": target,
            "memory": memory_hint,
        }

    agrees = bool(review.get("agrees", True))
    verdict = "agree" if agrees else ("reroute" if review.get("suggested_cli") or review.get("suggested_backend") else "warn")
    change = "/".join(part for part in [review.get("suggested_cli"), review.get("suggested_backend")] if part) or "none"
    return {
        "verdict": verdict,
        "confidence": "medium" if agrees else "high",
        "why": str(review.get("summary", "No review summary.")),
        "change": change,
        "memory": "consider",
    }


def _hexapus_lines(verdict: str, tick: int) -> list[str]:
    if verdict in {"block", "failed"}:
        return HEXAPUS_FRAMES[4]
    if verdict == "reroute":
        return HEXAPUS_FRAMES[tick % 4]
    if verdict == "warn":
        return HEXAPUS_FRAMES[(tick // 2) % 4]
    return HEXAPUS_FRAMES[(tick // 3) % 2]


def _init_colors() -> dict[str, int]:
    palette = {
        "base": 0,
        "accent": 0,
        "muted": 0,
        "warn": 0,
        "good": 0,
        "bad": 0,
        "hot": 0,
        "user": 0,
        "system": 0,
        "buddy": 0,
    }
    if not curses.has_colors():
        return palette
    curses.start_color()
    try:
        curses.use_default_colors()
    except curses.error:
        pass
    curses.init_pair(1, curses.COLOR_CYAN, -1)
    curses.init_pair(2, curses.COLOR_BLUE, -1)
    curses.init_pair(3, curses.COLOR_YELLOW, -1)
    curses.init_pair(4, curses.COLOR_GREEN, -1)
    curses.init_pair(5, curses.COLOR_RED, -1)
    curses.init_pair(6, curses.COLOR_MAGENTA, -1)
    curses.init_pair(7, curses.COLOR_WHITE, -1)
    palette = {
        "base": curses.color_pair(1),
        "accent": curses.color_pair(6) | curses.A_BOLD,
        "muted": curses.color_pair(2),
        "warn": curses.color_pair(3) | curses.A_BOLD,
        "good": curses.color_pair(4) | curses.A_BOLD,
        "bad": curses.color_pair(5) | curses.A_BOLD,
        "hot": curses.color_pair(6) | curses.A_BOLD,
        "user": curses.color_pair(7) | curses.A_BOLD,
        "system": curses.color_pair(3),
        "buddy": curses.color_pair(4),
    }
    return palette


def _verdict_attr(review: dict[str, str], colors: dict[str, int]) -> int:
    verdict = review["verdict"]
    if verdict in {"agree", "done"}:
        return colors["good"]
    if verdict in {"reroute", "warn"}:
        return colors["warn"]
    if verdict in {"block", "failed"}:
        return colors["bad"]
    return colors["accent"]


def _status_tag(status: str) -> str:
    return {
        "done": "DONE",
        "planned": "PLAN",
        "running": "RUN ",
        "failed": "FAIL",
        "needs_human": "HELP",
        "pending": "PEND",
        "draft": "DRAF",
    }.get(status, status[:4].upper())


def _role_state(machine: dict[str, Any], role: str) -> str:
    pane = machine.get("roles", {}).get(role)
    if not pane:
        return ".."
    if pane.get("dead"):
        return "xx"
    if pane.get("active"):
        return ">>"
    return "ok"


def _render_role_token(state: str, selected: bool, focused: bool) -> str:
    if selected and focused:
        return f"*{state}*"
    if selected:
        return f"<{state}>"
    if focused:
        return f"{{{state}}}"
    return f"[{state}]"


def _message_attr(role: str, colors: dict[str, int]) -> int:
    if role == "user":
        return colors["user"]
    if role == "system":
        return colors["system"]
    if role == "buddy":
        return colors["buddy"]
    if role == "constant":
        return colors["accent"]
    return colors["base"]


def _thread_preview(workspace: str, mission_id: str | None = None) -> str:
    history = read_chat_history(workspace=workspace, mission_id=mission_id, limit=1)
    if not history:
        return "Start a conversation"
    last = history[-1]
    content = str(last.get("content", "")).strip() or "(empty)"
    role = CHAT_LABELS.get(str(last.get("role", "system")), str(last.get("role", "system")).upper())
    return f"{role}: {content}"


def _streamed_text(state: TuiState, mission_id: str | None, text: str) -> str:
    if not state.stream_text or state.stream_text != text:
        return text
    if state.stream_mission_id != mission_id:
        return text
    elapsed = max(0.0, time.time() - state.stream_started_at)
    if elapsed >= state.stream_duration:
        return text
    total = max(1, len(text))
    progress = max(1, min(total, int(total * (elapsed / max(0.1, state.stream_duration)))))
    return text[:progress]


def _public_skill_menu() -> list[dict[str, Any]]:
    return list_skills(include_internal=False)


def _skill_menu_candidates(input_buffer: str) -> list[dict[str, Any]]:
    if not input_buffer.startswith("/"):
        return []
    payload = input_buffer[1:]
    if not payload:
        return _public_skill_menu()
    if payload.startswith("skill "):
        query = payload[len("skill ") :].strip().lower()
        if " " in query:
            return []
    else:
        stripped = payload.strip().lower()
        if " " in stripped:
            return []
        query = stripped
    skills = _public_skill_menu()
    if not query:
        return skills
    ranked: list[tuple[int, dict[str, Any]]] = []
    for skill in skills:
        score = 0
        skill_id = str(skill["id"]).lower()
        label = str(skill["label"]).lower()
        summary = str(skill["summary"]).lower()
        aliases = " ".join(str(alias).lower() for alias in skill.get("aliases", []))
        if skill_id.startswith(query):
            score += 5
        if query in skill_id:
            score += 3
        if query in label:
            score += 2
        if query in aliases:
            score += 2
        if query in summary:
            score += 1
        if score > 0:
            ranked.append((score, skill))
    ranked.sort(key=lambda item: (-item[0], item[1]["id"]))
    return [item[1] for item in ranked] or skills


def _slash_menu_pending(input_buffer: str) -> bool:
    return input_buffer.startswith("/") and bool(_skill_menu_candidates(input_buffer))


def _complete_slash_skill(input_buffer: str, selection: dict[str, Any]) -> str:
    if input_buffer.startswith("/skill "):
        return f"/skill {selection['id']} "
    return f"/{selection['id']} "


def _draw_skill_menu(
    stdscr: Any,
    y: int,
    x: int,
    width: int,
    candidates: list[dict[str, Any]],
    selected_index: int,
    colors: dict[str, int],
) -> None:
    if not candidates or width < 28:
        return
    visible = candidates[: min(5, len(candidates))]
    height = len(visible) + 3
    _draw_box(stdscr, y, x, height, width, "Skill Menu", colors["accent"])
    _safe_addstr(stdscr, y + 1, x + 2, "Tab/↑↓ browse | Enter complete", colors["muted"])
    row = y + 2
    selected = max(0, min(selected_index, len(visible) - 1))
    for index, skill in enumerate(visible):
        marker = ">" if index == selected else " "
        skill_id = str(skill["id"])
        summary = str(skill["summary"])
        line = f"{marker} /{skill_id:<24} {summary}"
        attr = colors["accent"] if index == selected else colors["base"]
        _safe_addstr(stdscr, row, x + 1, _clip(line, width - 3), attr)
        row += 1


def _draw_header(
    stdscr: Any,
    workspace: str,
    mission_count: int,
    fleet_count: int,
    runtime: dict[str, Any],
    memory: dict[str, Any],
    health: dict[str, Any],
    colors: dict[str, int],
    chat_scope_label: str,
    selected_machine_label: str | None,
    selected_role: str,
) -> None:
    memory_counts = memory.get("counts", {})
    planner_backend = health.get("models", {}).get("planner", {}).get("backend", "heuristic")
    buddy_backend = health.get("models", {}).get("buddy", {}).get("backend", "heuristic")
    cockpit_state = "up" if runtime.get("fleet_session_exists") else "down"
    focused = f"{runtime.get('focused_machine') or '-'}:{runtime.get('focused_role') or '-'}"
    selected = f"{selected_machine_label or '-'}:{selected_role}"
    _safe_addstr(stdscr, 0, 2, "Constant :: chat-first fleet cockpit", colors["accent"])
    _safe_addstr(
        stdscr,
        1,
        2,
        _clip(
            f"workspace={workspace}  scope={chat_scope_label}  cockpit={cockpit_state}  fleet={fleet_count}  threads={mission_count}",
            max(20, stdscr.getmaxyx()[1] - 4),
        ),
        colors["base"],
    )
    _safe_addstr(
        stdscr,
        2,
        2,
        _clip(
            f"planner={planner_backend}  buddy={buddy_backend}  selected={selected}  focused={focused}  docs={memory_counts.get('documents', 0)}  decisions={memory_counts.get('decisions', 0)}",
            max(20, stdscr.getmaxyx()[1] - 4),
        ),
        colors["muted"],
    )
    memory_error = memory.get("error")
    if memory_error:
        _safe_addstr(
            stdscr,
            3,
            2,
            _clip(memory_error, max(20, stdscr.getmaxyx()[1] - 4)),
            colors["bad"],
        )


def _draw_missions(stdscr: Any, y: int, x: int, height: int, width: int, missions: list[dict[str, Any]], selected: int, colors: dict[str, int]) -> None:
    _draw_box(stdscr, y, x, height, width, "Threads", colors["accent"])
    row = y + 1
    if not missions:
        _safe_addstr(stdscr, row, x + 2, "No missions yet. Start typing.", colors["muted"])
        return
    for index, mission in enumerate(missions[: max(1, height - 2)]):
        marker = ">" if index == selected else " "
        status = _status_tag(mission.get("status", "unknown"))
        line = f"{marker} {status} {mission['mission_id'][:6]} {mission['title']}"
        attr = colors["accent"] if index == selected else colors["base"]
        _safe_addstr(stdscr, row, x + 1, _clip(line, width - 3), attr)
        row += 1
        if row >= y + height - 1:
            break


def _draw_threads_focus(
    stdscr: Any,
    y: int,
    x: int,
    height: int,
    width: int,
    workspace: str,
    missions: list[dict[str, Any]],
    selected: int,
    colors: dict[str, int],
) -> None:
    _safe_addstr(stdscr, y, x + 1, "Chats", colors["accent"])
    row = y + 2
    _safe_addstr(stdscr, row, x + 1, _clip("global", width - 2), colors["hot"])
    row += 1
    _safe_addstr(stdscr, row, x + 2, _clip(_thread_preview(workspace), width - 3), colors["muted"])
    row += 2
    if not missions:
        _safe_addstr(stdscr, row, x + 1, "No mission threads yet.", colors["muted"])
        return
    for index, mission in enumerate(missions[: max(1, height - 6)]):
        marker = "›" if index == selected else " "
        title = f"{marker} {mission['title']}"
        attr = colors["accent"] if index == selected else colors["base"]
        _safe_addstr(stdscr, row, x + 1, _clip(title, width - 2), attr)
        row += 1
        _safe_addstr(
            stdscr,
            row,
            x + 2,
            _clip(_thread_preview(mission.get("workspace", workspace), mission["mission_id"]), width - 3),
            colors["muted"],
        )
        row += 2
        if row >= y + height - 1:
            break


def _draw_conversation(
    stdscr: Any,
    y: int,
    x: int,
    height: int,
    width: int,
    entries: list[dict[str, Any]],
    mission: dict[str, Any] | None,
    state: TuiState,
    colors: dict[str, int],
    focus_mode: bool = False,
) -> None:
    if mission and mission.get("steps"):
        step = next((entry for entry in mission["steps"] if entry.get("status") not in {"done", "failed"}), mission["steps"][0])
        scope = f"{mission['title']} :: {step.get('skill', '-')} / {step.get('agent', '-')}"
        compact_scope = f"{mission['title']}  ·  {step.get('skill', '-')} / {step.get('agent', '-')}"
    else:
        scope = f"mission {mission['mission_id'][:6]}" if mission else "workspace"
        compact_scope = scope
    if focus_mode:
        _safe_addstr(stdscr, y, x + 1, "Constant", colors["accent"])
        _safe_addstr(stdscr, y + 1, x + 1, _clip(compact_scope, width - 2), colors["muted"])
        row = y + 3
    else:
        _draw_box(stdscr, y, x, height, width, f"Constant :: {scope}", colors["accent"])
        row = y + 1
    if not entries:
        hints = [
            "No conversation yet.",
            "",
            "Type directly to talk to Constant.",
            "Use / for workflow skills.",
            "Actionable prompts create missions immediately.",
        ]
        _write_wrapped(stdscr, row, x + 2, width - 4, hints, colors["muted"])
        return

    rendered: list[tuple[str, int]] = []
    mission_id = mission.get("mission_id") if mission else None
    for entry in entries:
        role = str(entry.get("role", "system"))
        label = CHAT_LABELS.get(role, role.upper())
        timestamp = str(entry.get("timestamp", ""))[-9:-1]
        meta = entry.get("meta") or {}
        skill_tag = str(meta.get("skill", "")).strip()
        badge = f"[{skill_tag}] " if skill_tag else ""
        prefix = f"{timestamp} {label:<8}> {badge}" if not focus_mode else f"{label.lower():>8} "
        content = str(entry.get("content", "")).strip() or "(empty)"
        if role == "constant":
            content = _streamed_text(state, mission_id, content)
        wrapped = textwrap.wrap(content, max(10, width - 4 - len(prefix)), subsequent_indent=" " * len(prefix)) or [content]
        rendered.append((prefix + wrapped[0], _message_attr(role, colors)))
        for line in wrapped[1:]:
            rendered.append((" " * len(prefix) + line.lstrip(), _message_attr(role, colors)))
        if meta.get("created_mission_id"):
            rendered.append((f"{' ' * len(prefix)}-> mission {meta['created_mission_id']}", colors["system"]))
        route_bits = [str(meta.get("agent", "")).strip(), str(meta.get("cli", "")).strip()]
        route = " / ".join(part for part in route_bits if part)
        if route:
            rendered.append((f"{' ' * len(prefix)}-> route {route}", colors["muted"]))

    visible = rendered[-max(1, height - 2):]
    for line, attr in visible:
        if row >= y + height - 1:
            break
        _safe_addstr(stdscr, row, x + 2, _clip(line, width - 4), attr)
        row += 1


def _draw_runtime(
    stdscr: Any,
    y: int,
    x: int,
    height: int,
    width: int,
    runtime: dict[str, Any],
    mission: dict[str, Any] | None,
    selected_machine: int,
    selected_role: str,
    colors: dict[str, int],
) -> None:
    _draw_box(stdscr, y, x, height, width, "Fleet Context", colors["accent"])
    row = y + 1
    machines = runtime.get("machines", [])
    if not machines:
        _safe_addstr(stdscr, row, x + 2, "No runtime data yet. Open the cockpit with z.", colors["muted"])
        return

    header = "sel live machine            claude codex copilot vibe  state    target"
    _safe_addstr(stdscr, row, x + 2, _clip(header, width - 4), colors["hot"])
    row += 1

    mission_steps = mission.get("steps", []) if mission else []
    for index, machine in enumerate(machines[: max(1, height - 3)]):
        selected = index == selected_machine
        focused = machine["label"] == runtime.get("focused_machine")
        marker = ">" if selected else " "
        live = "*" if focused else " "
        attr = colors["accent"] if selected else (colors["hot"] if focused else colors["base"])
        status = "up" if machine.get("session_exists") else "down"
        role_bits = []
        for role in ROLES:
            state = _role_state(machine, role)
            token = _render_role_token(state, selected and role == selected_role, focused and role == runtime.get("focused_role"))
            role_bits.append(token)
        line = f"{marker}   {live} {machine['label']:<16} {' '.join(role_bits)}  {status:<7} {machine['target']}"
        _safe_addstr(stdscr, row, x + 1, _clip(line, width - 3), attr)
        row += 1
        matching = sorted(
            (step for step in mission_steps if step.get("machine") == machine["label"]),
            key=lambda step: (step.get("status") in {"done", "failed"}, step.get("attempt", 0)),
        )
        for step in matching[:1]:
            route_line = (
                f"      route {step.get('cli', '-')}/{step.get('backend', '-')}  "
                f"skill={step.get('skill', '-')}  agent={step.get('agent', '-')}"
            )
            route_attr = colors["muted"] if not selected else colors["good"]
            if row < y + height - 1:
                _safe_addstr(stdscr, row, x + 1, _clip(route_line, width - 3), route_attr)
                row += 1
        if row >= y + height - 1:
            break


def _draw_capture_view(
    stdscr: Any,
    y: int,
    x: int,
    height: int,
    width: int,
    capture_state: dict[str, Any],
    colors: dict[str, int],
) -> None:
    title = f"Capture View :: {capture_state['machine']}:{capture_state['pane']}"
    _draw_box(stdscr, y, x, height, width, title, colors["accent"])
    lines = capture_state.get("lines", []) or ["(empty capture)"]
    row = y + 1
    body_height = max(1, height - 2)
    max_scroll = max(0, len(lines) - body_height)
    scroll = max(0, min(capture_state.get("scroll", 0), max_scroll))
    capture_state["scroll"] = scroll
    status_bits = [f"scroll {scroll + 1}/{max(1, max_scroll + 1)}"]
    if capture_state.get("returncode"):
        status_bits.append(f"rc={capture_state['returncode']}")
    _safe_addstr(stdscr, row, x + 2, _clip(" | ".join(status_bits), width - 4), colors["muted"])
    row += 1
    visible = lines[scroll : scroll + max(1, body_height - 1)]
    for line in visible:
        if row >= y + height - 1:
            break
        attr = colors["bad"] if capture_state.get("returncode") else colors["base"]
        _safe_addstr(stdscr, row, x + 2, _clip(line, width - 4), attr)
        row += 1


def _draw_buddy(
    stdscr: Any,
    y: int,
    x: int,
    height: int,
    width: int,
    review: dict[str, str],
    memory: dict[str, Any],
    colors: dict[str, int],
    tick: int,
) -> None:
    _draw_box(stdscr, y, x, height, width, "Buddy", colors["accent"])
    row = y + 1
    verdict_attr = _verdict_attr(review, colors)
    for line in _hexapus_lines(review["verdict"], tick):
        if row >= y + height - 1:
            return
        _safe_addstr(stdscr, row, x + 2, _clip(line, width - 4), verdict_attr)
        row += 1
    row += 1
    for line in [
        f"verdict:    {review['verdict']}",
        f"confidence: {review['confidence']}",
        f"change:     {review['change']}",
        f"memory:     {review['memory']}",
    ]:
        if row >= y + height - 1:
            return
        _safe_addstr(stdscr, row, x + 2, _clip(line, width - 4), colors["base"])
        row += 1
    row += 1
    row = _write_wrapped(stdscr, row, x + 2, width - 4, [review["why"]], colors["muted"])
    row += 1
    counts = memory.get("counts", {})
    for line in [
        f"persona facts : {counts.get('persona_facts', 0)}",
        f"decisions     : {counts.get('decisions', 0)}",
        f"mission notes : {counts.get('mission_summaries', 0)}",
    ]:
        if row >= y + height - 1:
            return
        _safe_addstr(stdscr, row, x + 2, _clip(line, width - 4), colors["base"])
        row += 1


def _draw_context_rail(
    stdscr: Any,
    y: int,
    x: int,
    height: int,
    width: int,
    review: dict[str, str],
    runtime: dict[str, Any],
    mission: dict[str, Any] | None,
    selected_machine_label: str | None,
    selected_role: str,
    colors: dict[str, int],
    tick: int,
) -> None:
    _draw_box(stdscr, y, x, height, width, "Live", colors["accent"])
    row = y + 1
    verdict_attr = _verdict_attr(review, colors)
    for line in _hexapus_lines(review["verdict"], tick)[:8]:
        if row >= y + height - 1:
            return
        _safe_addstr(stdscr, row, x + 2, _clip(line, width - 4), verdict_attr)
        row += 1
    row += 1
    lines = [
        f"buddy     {review['verdict']} / {review['confidence']}",
        f"selected  {selected_machine_label or '-'}:{selected_role}",
        f"focused   {runtime.get('focused_machine') or '-'}:{runtime.get('focused_role') or '-'}",
    ]
    if mission and mission.get("steps"):
        step = next((entry for entry in mission["steps"] if entry.get("status") not in {"done", "failed"}), mission["steps"][0])
        lines.extend(
            [
                f"skill     {step.get('skill', '-')}",
                f"agent     {step.get('agent', '-')}",
                f"route     {step.get('cli', '-')}/{step.get('backend', '-')}",
            ]
        )
    for line in lines:
        if row >= y + height - 1:
            return
        _safe_addstr(stdscr, row, x + 2, _clip(line, width - 4), colors["base"])
        row += 1
    row += 1
    if row < y + height - 1:
        _write_wrapped(stdscr, row, x + 2, width - 4, [review["why"]], colors["muted"])


def _draw_timeline(stdscr: Any, y: int, x: int, height: int, width: int, events: list[str], persona_lines: list[str], decision_lines: list[str], colors: dict[str, int]) -> None:
    _draw_box(stdscr, y, x, height, width, "Timeline + Memory", colors["accent"])
    half = max(20, width // 2)
    _safe_addstr(stdscr, y + 1, x + 2, "events", colors["hot"])
    _safe_addstr(stdscr, y + 1, x + half, "memory echoes", colors["hot"])
    row_left = y + 2
    for event in events[: max(1, height - 3)]:
        _safe_addstr(stdscr, row_left, x + 2, _clip(event, half - 4), colors["base"])
        row_left += 1
        if row_left >= y + height - 1:
            break
    row_right = y + 2
    memory_lines = [*persona_lines[:3], *decision_lines[:3]]
    for line in memory_lines[: max(1, height - 3)]:
        _safe_addstr(stdscr, row_right, x + half, _clip(line, width - half - 2), colors["muted"])
        row_right += 1
        if row_right >= y + height - 1:
            break


def _compute_layout(height: int, width: int, chat_focus: bool) -> TuiLayout:
    top = 4
    footer_height = 2
    timeline_height = 0 if chat_focus else min(6, max(4, height // 8))
    main_height = max(12, height - top - timeline_height - footer_height)
    if chat_focus:
        left_width = max(20, min(24, width // 6))
        right_width = max(22, min(28, width // 5))
    else:
        left_width = max(22, min(28, width // 5))
        right_width = max(24, min(30, width // 4))
    center_width = max(28, width - left_width - right_width - 4)
    conversation_height = main_height if chat_focus else max(10, int(main_height * 0.74))
    runtime_height = 0 if chat_focus else max(6, main_height - conversation_height)
    return TuiLayout(
        height=height,
        width=width,
        top=top,
        footer_height=footer_height,
        timeline_height=timeline_height,
        main_height=main_height,
        left_width=left_width,
        right_width=right_width,
        center_width=center_width,
        conversation_height=conversation_height,
        runtime_height=runtime_height,
    )


def _collect_snapshot(default_workspace: str, selected_index: int, local_session: str, machine_session: str, runtime_snapshot: dict[str, Any] | None = None) -> dict[str, Any]:
    missions = sorted(list_missions(), key=lambda item: item.get("updated_at", ""), reverse=True)
    if missions:
        selected_index = max(0, min(selected_index, len(missions) - 1))
        selected_mission = missions[selected_index]
        workspace = str(selected_mission.get("workspace", default_workspace))
    else:
        selected_mission = None
        workspace = default_workspace

    health = daemon_request("health", auto_start=False)
    memory = _safe_memory_status(workspace)
    review = _normalize_buddy(selected_mission.get("buddy_review") if selected_mission else None)
    persona_lines = _safe_persona_lines()
    decision_lines = _safe_decision_lines(workspace)
    events = _recent_events(selected_mission["mission_id"] if selected_mission else None)
    fleet = load_fleet_config()
    runtime = runtime_snapshot if runtime_snapshot is not None else _safe_runtime_status(local_session, machine_session)
    chat_history = read_chat_history(workspace=workspace, mission_id=selected_mission["mission_id"] if selected_mission else None, limit=80)
    chat_scope_label = f"mission:{selected_mission['mission_id'][:6]}" if selected_mission else "workspace"
    return {
        "missions": missions,
        "selected_index": selected_index,
        "selected_mission": selected_mission,
        "workspace": workspace,
        "health": health,
        "memory": memory,
        "review": review,
        "persona_lines": persona_lines,
        "decision_lines": decision_lines,
        "events": events,
        "fleet_count": len(fleet["machines"]),
        "runtime": runtime,
        "chat_history": chat_history,
        "chat_scope_label": chat_scope_label,
    }


def _ensure_mission_planned(mission: dict[str, Any]) -> dict[str, Any]:
    if mission["steps"]:
        return mission
    payload = daemon_request("plan", {"mission": mission})
    mission["steps"] = payload["plan"]["steps"]
    mission["title"] = payload["plan"]["title"]
    mission["status"] = "planned"
    mission["planner_summary"] = payload["plan"]["summary"]
    mission["buddy_review"] = payload["buddy_review"]
    save_mission(mission)
    append_event(mission["mission_id"], "mission.planned", payload)
    return mission


def _selected_machine_label(snapshot: dict[str, Any], selected_machine: int) -> str | None:
    machines = snapshot["runtime"].get("machines", [])
    if not machines:
        return None
    return machines[selected_machine]["label"]


def _open_capture(machine: str, pane: str, machine_session: str, current_height: int, current_scroll: int | None = None) -> dict[str, Any]:
    capture = capture_pane(machine, pane, lines=200, machine_session=machine_session)
    if capture.get("returncode", 0) != 0:
        error_text = capture.get("stderr") or capture.get("stdout") or "capture command failed"
        lines = [line for line in error_text.splitlines() if line] or ["capture command failed"]
    else:
        text = capture.get("stdout") or ""
        lines = text.splitlines() or ["(empty capture)"]
    view_height = max(8, current_height - 2)
    scroll = max(0, len(lines) - view_height) if current_scroll is None else min(max(0, current_scroll), max(0, len(lines) - view_height))
    return {
        "machine": machine,
        "pane": pane,
        "lines": lines,
        "scroll": scroll,
        "returncode": int(capture.get("returncode", 0)),
        "raw": capture,
    }


def _process_chat_submission(
    text: str,
    snapshot: dict[str, Any],
    selected_machine: int,
    selected_role: str,
    local_session: str,
    machine_session: str,
    runtime_height: int,
) -> dict[str, Any]:
    message = text.strip()
    if not message:
        return {
            "flash": "empty prompt",
            "force_select_mission_id": None,
            "capture_state": None,
            "invalidate_runtime": False,
            "next_action": None,
        }

    mission = snapshot["selected_mission"]
    mission_id = mission["mission_id"] if mission else None
    workspace = snapshot["workspace"]
    machine_label = _selected_machine_label(snapshot, selected_machine)

    append_chat_message(
        "user",
        message,
        workspace=workspace,
        mission_id=mission_id,
        machine=machine_label,
        pane=selected_role,
    )
    if mission_id:
        append_event(mission_id, "chat.user", {"content": message, "machine": machine_label, "pane": selected_role})

    payload = daemon_request(
        "chat",
        {
            "message": message,
            "mission": mission,
            "workspace": workspace,
            "selected_machine": machine_label,
            "selected_role": selected_role,
            "chat_history": snapshot["chat_history"][-12:],
        },
    )

    reply = str(payload.get("reply", "")).strip() or "No reply."
    buddy_note = payload.get("buddy_note")
    intent = payload.get("intent", "plain_chat")
    action = payload.get("cockpit_action")
    requested_skill = payload.get("skill") or {}
    requested_skill_id = str(requested_skill.get("id", "")).strip()
    routing_overrides = payload.get("routing_overrides") or {}

    capture_state = None
    next_action = None
    force_select_mission_id = None
    thread_mission_id = mission_id

    if intent == "mission_create":
        mission = create_mission(
            payload.get("mission_goal") or message,
            workspace,
            routing_overrides=routing_overrides,
        )
        mission = _ensure_mission_planned(mission)
        force_select_mission_id = mission["mission_id"]
        step = mission["steps"][0] if mission.get("steps") else {}
        step_skill = step.get("skill", requested_skill_id)
        append_chat_message(
            "system",
            f"Promoted to mission {mission['mission_id']} :: {mission['title']}",
            workspace=workspace,
            mission_id=mission_id,
            intent="mission_create",
            machine=machine_label,
            pane=selected_role,
            meta={
                "created_mission_id": mission["mission_id"],
                "skill": step_skill,
                "agent": step.get("agent"),
                "cli": step.get("cli"),
            },
        )
        append_chat_message(
            "user",
            message,
            workspace=workspace,
            mission_id=mission["mission_id"],
            intent="mission_create",
            machine=machine_label,
            pane=selected_role,
            meta={"skill": step_skill},
        )
        append_chat_message(
            "constant",
            reply,
            workspace=workspace,
            mission_id=mission["mission_id"],
            intent="mission_create",
            machine=machine_label,
            pane=selected_role,
            meta={
                "created_mission_id": mission["mission_id"],
                "skill": step_skill,
                "agent": step.get("agent"),
                "cli": step.get("cli"),
            },
        )
        if buddy_note:
            append_chat_message(
                "buddy",
                str(buddy_note.get("answer", "")),
                workspace=workspace,
                mission_id=mission["mission_id"],
                intent="buddy_answer",
                meta={"skill": step_skill},
            )
        append_event(mission["mission_id"], "chat.constant", {"intent": intent, "reply": reply})
        return {
            "flash": f"mission created {mission['mission_id']} -> {mission['title']} [{step.get('skill', '-')} / {step.get('agent', '-')}]",
            "force_select_mission_id": force_select_mission_id,
            "capture_state": None,
            "invalidate_runtime": False,
            "stream_text": reply,
            "stream_mission_id": mission["mission_id"],
            "next_action": None,
        }

    append_chat_message(
        "constant",
        reply,
        workspace=workspace,
        mission_id=thread_mission_id,
        intent=intent,
        machine=machine_label,
        pane=selected_role,
        meta={
            "memory_hits": payload.get("memory_hits", [])[:3],
            "skill": requested_skill_id,
            "agent": routing_overrides.get("agent"),
            "cli": routing_overrides.get("cli"),
        },
    )
    if thread_mission_id:
        append_event(thread_mission_id, "chat.constant", {"intent": intent, "reply": reply})

    if buddy_note:
        append_chat_message(
            "buddy",
            str(buddy_note.get("answer", "")),
            workspace=workspace,
            mission_id=thread_mission_id,
            intent="buddy_answer",
            meta={"skill": requested_skill_id},
        )

    if action:
        action_type = action.get("type")
        action_machine = action.get("machine") or machine_label
        action_pane = action.get("pane") or selected_role
        try:
            if action_type == "focus" and action_machine:
                focus_machine(action_machine, action_pane, local_session=local_session, machine_session=machine_session)
                append_chat_message("system", f"Focused {action_machine}:{action_pane}", workspace=workspace, mission_id=thread_mission_id, intent=intent)
                return {
                    "flash": f"focused {action_machine}:{action_pane}",
                    "force_select_mission_id": None,
                    "capture_state": None,
                    "invalidate_runtime": True,
                    "next_action": None,
                }
            if action_type == "restart" and action_machine:
                restart_pane(action_machine, action_pane, machine_session=machine_session)
                append_chat_message("system", f"Restart sent to {action_machine}:{action_pane}", workspace=workspace, mission_id=thread_mission_id, intent=intent)
                return {
                    "flash": f"restart sent to {action_machine}:{action_pane}",
                    "force_select_mission_id": None,
                    "capture_state": None,
                    "invalidate_runtime": True,
                    "next_action": None,
                }
            if action_type == "capture" and action_machine:
                capture_state = _open_capture(action_machine, action_pane, machine_session, runtime_height)
                append_chat_message("system", f"Captured {action_machine}:{action_pane}", workspace=workspace, mission_id=thread_mission_id, intent=intent)
                return {
                    "flash": f"capture loaded for {action_machine}:{action_pane}",
                    "force_select_mission_id": None,
                    "capture_state": capture_state,
                    "invalidate_runtime": False,
                    "next_action": None,
                }
            if action_type == "open":
                append_chat_message("system", "Opening full cockpit view", workspace=workspace, mission_id=thread_mission_id, intent=intent)
                return {
                    "flash": "opening full cockpit",
                    "force_select_mission_id": None,
                    "capture_state": None,
                    "invalidate_runtime": False,
                    "next_action": {"action": "cockpit", "workspace": workspace},
                }
        except Exception as exc:  # noqa: BLE001
            append_chat_message("system", _error_line("cockpit action failed", exc), workspace=workspace, mission_id=thread_mission_id, intent="cockpit_error")
            return {
                "flash": _error_line("cockpit action failed", exc),
                "force_select_mission_id": None,
                "capture_state": None,
                "invalidate_runtime": False,
                "next_action": None,
            }

    return {
        "flash": "message routed through Constant",
        "force_select_mission_id": None,
        "capture_state": None,
        "invalidate_runtime": False,
        "stream_text": reply,
        "stream_mission_id": thread_mission_id,
        "next_action": next_action,
    }


def _draw_status_and_prompt(
    stdscr: Any,
    height: int,
    width: int,
    *,
    flash: str,
    flash_until: float,
    input_mode: bool,
    input_buffer: str,
    capture_state: dict[str, Any] | None,
    snapshot: dict[str, Any],
    selected_machine_label: str | None,
    selected_role: str,
    colors: dict[str, int],
) -> None:
    now = time.time()
    if flash and now < flash_until:
        status = flash
        status_attr = colors["warn"]
    elif capture_state:
        status = "capture | j/k scroll | PgUp/PgDn page | c refresh | x close | q quit"
        status_attr = colors["muted"]
    elif input_mode:
        status = "chat | Enter send | / skills | Tab menu | Esc cockpit | Ctrl-U clear"
        status_attr = colors["muted"]
    else:
        focused = f"{snapshot['runtime'].get('focused_machine') or '-'}:{snapshot['runtime'].get('focused_role') or '-'}"
        selected = f"{selected_machine_label or '-'}:{selected_role}"
        view = "chat-focus" if snapshot.get("chat_focus") else "cockpit-detail"
        status = f"cockpit | type to chat | f view | [/] machine | 1..4 pane | o focus | x capture | r restart | z tabs | q quit | {view} | sel={selected} | live={focused}"
        status_attr = colors["muted"]

    prompt_prefix = "› "
    prompt_text = prompt_prefix + input_buffer
    if input_mode:
        prompt_text += "_"

    _safe_addstr(stdscr, height - 2, 2, _clip(status, width - 4), status_attr)
    _safe_addstr(stdscr, height - 1, 2, _clip(prompt_text, width - 4), colors["hot"] if input_mode else colors["base"])
    if input_mode:
        try:
            cursor_x = min(width - 2, 2 + len(prompt_prefix) + len(input_buffer))
            stdscr.move(height - 1, max(2, cursor_x))
        except curses.error:
            pass


def _refresh_snapshot(state: TuiState, workspace: str, local_session: str, machine_session: str) -> dict[str, Any]:
    now = time.time()
    if state.runtime_cache is None or now >= state.runtime_refresh_at:
        state.runtime_cache = _safe_runtime_status(local_session, machine_session)
        state.runtime_refresh_at = now + 2.0

    snapshot = _collect_snapshot(workspace, state.selected_index, local_session, machine_session, runtime_snapshot=state.runtime_cache)
    if state.pending_select_mission_id:
        for index, mission in enumerate(snapshot["missions"]):
            if mission["mission_id"] == state.pending_select_mission_id:
                state.selected_index = index
                snapshot = _collect_snapshot(workspace, state.selected_index, local_session, machine_session, runtime_snapshot=state.runtime_cache)
                break
        state.pending_select_mission_id = None

    state.selected_index = snapshot["selected_index"]
    runtime_machines = snapshot["runtime"].get("machines", [])
    if runtime_machines:
        state.selected_machine = max(0, min(state.selected_machine, len(runtime_machines) - 1))
    else:
        state.selected_machine = 0
    return snapshot


def _draw_frame(stdscr: Any, snapshot: dict[str, Any], state: TuiState, colors: dict[str, int]) -> TuiLayout:
    stdscr.erase()
    height, width = stdscr.getmaxyx()
    layout = _compute_layout(height, width, state.chat_focus and not state.capture_state)
    snapshot["chat_focus"] = state.chat_focus and not state.capture_state
    machine_label = _selected_machine_label(snapshot, state.selected_machine)

    _draw_header(
        stdscr,
        snapshot["workspace"],
        len(snapshot["missions"]),
        snapshot["fleet_count"],
        snapshot["runtime"],
        snapshot["memory"],
        snapshot["health"],
        colors,
        snapshot["chat_scope_label"],
        machine_label,
        state.selected_role,
    )
    if state.chat_focus and not state.capture_state:
        _draw_threads_focus(
            stdscr,
            layout.top,
            0,
            layout.main_height,
            layout.left_width,
            snapshot["workspace"],
            snapshot["missions"],
            state.selected_index,
            colors,
        )
    else:
        _draw_missions(stdscr, layout.top, 0, layout.main_height, layout.left_width, snapshot["missions"], state.selected_index, colors)
    _draw_conversation(
        stdscr,
        layout.top,
        layout.left_width + 1,
        layout.conversation_height,
        layout.center_width,
        snapshot["chat_history"],
        snapshot["selected_mission"],
        state,
        colors,
        state.chat_focus and not state.capture_state,
    )
    skill_candidates = _skill_menu_candidates(state.input_buffer) if state.input_mode else []
    if skill_candidates:
        menu_width = min(max(44, layout.center_width - 4), layout.center_width)
        _draw_skill_menu(
            stdscr,
            layout.top + 2,
            layout.left_width + 3,
            menu_width,
            skill_candidates,
            state.slash_menu_index,
            colors,
        )
    if state.capture_state:
        _draw_capture_view(
            stdscr,
            layout.top + layout.conversation_height,
            layout.left_width + 1,
            layout.runtime_height,
            layout.center_width,
            state.capture_state,
            colors,
        )
    elif not state.chat_focus:
        _draw_runtime(
            stdscr,
            layout.top + layout.conversation_height,
            layout.left_width + 1,
            layout.runtime_height,
            layout.center_width,
            snapshot["runtime"],
            snapshot["selected_mission"],
            state.selected_machine,
            state.selected_role,
            colors,
        )
    if state.chat_focus and not state.capture_state:
        _draw_context_rail(
            stdscr,
            layout.top,
            layout.left_width + layout.center_width + 2,
            layout.main_height,
            layout.right_width,
            snapshot["review"],
            snapshot["runtime"],
            snapshot["selected_mission"],
            machine_label,
            state.selected_role,
            colors,
            int(time.time() * 4),
        )
    else:
        _draw_buddy(
            stdscr,
            layout.top,
            layout.left_width + layout.center_width + 2,
            layout.main_height,
            layout.right_width,
            snapshot["review"],
            snapshot["memory"],
            colors,
            int(time.time() * 4),
        )
    if layout.timeline_height > 0:
        _draw_timeline(
            stdscr,
            layout.top + layout.main_height,
            0,
            layout.timeline_height,
            layout.width,
            snapshot["events"],
            snapshot["persona_lines"],
            snapshot["decision_lines"],
            colors,
        )
    _draw_status_and_prompt(
        stdscr,
        layout.height,
        layout.width,
        flash=state.flash,
        flash_until=state.flash_until,
        input_mode=state.input_mode,
        input_buffer=state.input_buffer,
        capture_state=state.capture_state,
        snapshot=snapshot,
        selected_machine_label=machine_label,
        selected_role=state.selected_role,
        colors=colors,
    )
    stdscr.refresh()
    return layout


def _handle_input_mode_key(
    key: int,
    state: TuiState,
    snapshot: dict[str, Any],
    local_session: str,
    machine_session: str,
    runtime_height: int,
) -> dict[str, Any] | None:
    if key in (10, 13, curses.KEY_ENTER):
        candidates = _skill_menu_candidates(state.input_buffer)
        if _slash_menu_pending(state.input_buffer) and candidates:
            selected = candidates[max(0, min(state.slash_menu_index, len(candidates) - 1))]
            state.input_buffer = _complete_slash_skill(state.input_buffer, selected)
            state.slash_menu_index = 0
            state.set_flash(f"selected /{selected['id']}", 1.5)
            return None
        outcome = _process_chat_submission(
            state.input_buffer,
            snapshot,
            state.selected_machine,
            state.selected_role,
            local_session,
            machine_session,
            runtime_height,
        )
        state.input_mode = True
        state.input_buffer = ""
        stream_text = outcome.get("stream_text", "")
        if stream_text:
            state.stream_text = stream_text
            state.stream_started_at = time.time()
            state.stream_duration = min(3.8, max(0.7, len(stream_text) / 72.0))
            state.stream_mission_id = outcome.get("stream_mission_id")
        else:
            state.stream_text = ""
            state.stream_duration = 0.0
            state.stream_mission_id = None
        if outcome["capture_state"] is not None:
            state.capture_state = outcome["capture_state"]
        if outcome["invalidate_runtime"]:
            state.runtime_cache = None
        if outcome["force_select_mission_id"]:
            state.pending_select_mission_id = outcome["force_select_mission_id"]
        state.set_flash(outcome["flash"], 3.0)
        return outcome["next_action"]
    if key == 27:
        state.input_mode = False
        state.input_buffer = ""
        state.slash_menu_index = 0
        state.set_flash("chat canceled", 1.5)
        return None
    candidates = _skill_menu_candidates(state.input_buffer)
    if candidates and key in (9, curses.KEY_DOWN):
        state.slash_menu_index = (state.slash_menu_index + 1) % len(candidates)
        return None
    if candidates and key == curses.KEY_UP:
        state.slash_menu_index = (state.slash_menu_index - 1) % len(candidates)
        return None
    if key in (curses.KEY_BACKSPACE, 127, 8):
        state.input_buffer = state.input_buffer[:-1]
        state.slash_menu_index = 0
        return None
    if key == 21:
        state.input_buffer = ""
        state.slash_menu_index = 0
        return None
    if 32 <= key <= 126:
        state.input_buffer += chr(key)
        state.slash_menu_index = 0
    return None


def _handle_capture_key(key: int, state: TuiState, machine_session: str, runtime_height: int) -> bool:
    if not state.capture_state:
        return False
    if key == ord("x"):
        state.capture_state = None
        state.set_flash("capture view closed", 1.5)
        return True
    if key == ord("c"):
        try:
            state.capture_state = _open_capture(
                state.capture_state["machine"],
                state.capture_state["pane"],
                machine_session,
                runtime_height,
                state.capture_state.get("scroll"),
            )
            if state.capture_state.get("returncode"):
                state.set_flash(
                    f"capture error for {state.capture_state['machine']}:{state.capture_state['pane']}",
                    2.5,
                )
            else:
                state.set_flash(
                    f"capture refreshed for {state.capture_state['machine']}:{state.capture_state['pane']}",
                    2.5,
                )
        except Exception as exc:  # noqa: BLE001
            state.set_flash(_error_line("capture refresh failed", exc), 2.5)
        return True
    if key in (ord("j"), curses.KEY_DOWN):
        max_scroll = max(0, len(state.capture_state.get("lines", [])) - max(1, runtime_height - 3))
        state.capture_state["scroll"] = min(max_scroll, state.capture_state.get("scroll", 0) + 1)
        return True
    if key in (ord("k"), curses.KEY_UP):
        state.capture_state["scroll"] = max(0, state.capture_state.get("scroll", 0) - 1)
        return True
    if key == curses.KEY_NPAGE:
        max_scroll = max(0, len(state.capture_state.get("lines", [])) - max(1, runtime_height - 3))
        state.capture_state["scroll"] = min(max_scroll, state.capture_state.get("scroll", 0) + max(4, runtime_height // 2))
        return True
    if key == curses.KEY_PPAGE:
        state.capture_state["scroll"] = max(0, state.capture_state.get("scroll", 0) - max(4, runtime_height // 2))
        return True
    if key == ord("/"):
        state.input_mode = True
        state.input_buffer = "/"
        state.slash_menu_index = 0
        return True
    return False


def _handle_normal_key(
    key: int,
    state: TuiState,
    snapshot: dict[str, Any],
    local_session: str,
    machine_session: str,
    runtime_height: int,
) -> dict[str, Any] | None:
    if key in (ord("q"), 27):
        return {"action": "quit"}
    if key == ord("/"):
        state.input_mode = True
        state.input_buffer = "/"
        state.slash_menu_index = 0
        return None
    if key == ord("f"):
        state.chat_focus = not state.chat_focus
        state.set_flash("chat-focus enabled" if state.chat_focus else "cockpit-detail enabled", 1.5)
        return None
    if key == ord("p"):
        state.input_mode = True
        state.input_buffer = "/spec-planner "
        state.slash_menu_index = 0
        state.set_flash("spec-planner primed", 1.5)
        return None
    if key == ord("b"):
        state.input_mode = True
        state.input_buffer = "/architecture-brainstorm "
        state.slash_menu_index = 0
        state.set_flash("architecture-brainstorm primed", 1.5)
        return None
    if key == ord("t"):
        state.input_mode = True
        state.input_buffer = "/task-decomposer "
        state.slash_menu_index = 0
        state.set_flash("task-decomposer primed", 1.5)
        return None
    if key == ord("P"):
        state.input_mode = True
        state.input_buffer = "/pr-review-prep "
        state.slash_menu_index = 0
        state.set_flash("pr-review-prep primed", 1.5)
        return None
    if 32 <= key <= 126 and chr(key) not in {"[", "]", "1", "2", "3", "4", "o", "r", "x", "z", "j", "k", "q", "e", "s", "c", "/", "p", "b", "t", "P", "f"}:
        state.input_mode = True
        state.input_buffer = chr(key)
        return None

    machines = snapshot["runtime"].get("machines", [])
    machine_label = _selected_machine_label(snapshot, state.selected_machine)

    if key in (ord("j"), curses.KEY_DOWN):
        if snapshot["missions"]:
            state.selected_index = min(len(snapshot["missions"]) - 1, state.selected_index + 1)
        return None
    if key in (ord("k"), curses.KEY_UP):
        if snapshot["missions"]:
            state.selected_index = max(0, state.selected_index - 1)
        return None
    if key == ord("["):
        if machines:
            state.selected_machine = max(0, state.selected_machine - 1)
        return None
    if key == ord("]"):
        if machines:
            state.selected_machine = min(len(machines) - 1, state.selected_machine + 1)
        return None
    if key in (ord("1"), ord("2"), ord("3"), ord("4")) and machines:
        state.selected_role_index = int(chr(key)) - 1
        state.set_flash(f"selected {machine_label}:{state.selected_role}", 1.5)
        return None
    if key == ord("o") and machines:
        machine = machines[state.selected_machine]
        try:
            focus_machine(
                machine["label"],
                state.selected_role,
                local_session=local_session,
                machine_session=machine_session,
            )
            state.runtime_cache = None
            state.set_flash(f"focused {machine['label']}:{state.selected_role}", 2.5)
        except Exception as exc:  # noqa: BLE001
            state.set_flash(_error_line("focus failed", exc), 2.5)
        return None
    if key == ord("r") and machines:
        machine = machines[state.selected_machine]
        try:
            restart_pane(machine["label"], state.selected_role, machine_session=machine_session)
            state.runtime_cache = None
            state.set_flash(f"restart sent to {machine['label']}:{state.selected_role}", 2.5)
        except Exception as exc:  # noqa: BLE001
            state.set_flash(_error_line("restart failed", exc), 2.5)
        return None
    if key == ord("x") and machines:
        machine = machines[state.selected_machine]
        try:
            state.capture_state = _open_capture(machine["label"], state.selected_role, machine_session, runtime_height)
            if state.capture_state.get("returncode"):
                state.set_flash(f"capture error for {machine['label']}:{state.selected_role}", 2.5)
            else:
                state.set_flash(f"capture loaded for {machine['label']}:{state.selected_role}", 2.5)
        except Exception as exc:  # noqa: BLE001
            state.set_flash(_error_line("capture failed", exc), 2.5)
        return None
    if key == ord("z"):
        return {"action": "cockpit", "workspace": snapshot["workspace"]}
    if key == ord("e"):
        try:
            rebuild_workspace_memory(snapshot["workspace"], enroll=True)
            state.set_flash(f"memory rebuilt for {snapshot['workspace']}", 2.5)
        except Exception as exc:  # noqa: BLE001
            state.set_flash(_error_line("memory rebuild failed", exc), 2.5)
        return None
    if key == ord("s") and snapshot["selected_mission"]:
        try:
            summarize_mission(snapshot["selected_mission"]["mission_id"])
            state.set_flash(f"mission summarized: {snapshot['selected_mission']['mission_id']}", 2.5)
        except Exception as exc:  # noqa: BLE001
            state.set_flash(_error_line("summary failed", exc), 2.5)
        return None
    return None


def _run(stdscr: Any, workspace: str, local_session: str, machine_session: str) -> dict[str, Any] | None:
    try:
        curses.curs_set(1)
    except curses.error:
        pass
    stdscr.nodelay(False)
    stdscr.timeout(250)
    colors = _init_colors()
    state = TuiState()

    while True:
        snapshot = _refresh_snapshot(state, workspace, local_session, machine_session)
        layout = _draw_frame(stdscr, snapshot, state, colors)

        key = stdscr.getch()
        if key in (-1, curses.KEY_RESIZE):
            continue

        if state.input_mode:
            next_action = _handle_input_mode_key(
                key,
                state,
                snapshot,
                local_session,
                machine_session,
                layout.runtime_height,
            )
            if next_action:
                return next_action
            continue

        if _handle_capture_key(key, state, machine_session, layout.runtime_height):
            continue

        action = _handle_normal_key(
            key,
            state,
            snapshot,
            local_session,
            machine_session,
            layout.runtime_height,
        )
        if action and action.get("action") == "quit":
            return None
        if action:
            return action


def run_tui(workspace: str, local_session: str = "constant-fleet", machine_session: str = "constant") -> dict[str, Any] | None:
    try:
        prime_workspace_memory(workspace, enroll=True)
    except Exception:
        pass
    return curses.wrapper(lambda stdscr: _run(stdscr, str(Path(workspace).expanduser().resolve()), local_session, machine_session))
