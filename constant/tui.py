from __future__ import annotations

import curses
import json
import textwrap
import time
from pathlib import Path
from typing import Any

from .cockpit import ROLES, capture_pane, focus_machine, restart_pane, runtime_status
from .daemon import request as daemon_request
from .memory import list_decisions, memory_status, persona_markdown, prime_workspace_memory, rebuild_workspace_memory, summarize_mission
from .state import list_missions, load_fleet_config, mission_events_file


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


def _hexapus_lines() -> list[str]:
    return [
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
    ]


def _init_colors() -> dict[str, int]:
    palette = {"base": 0, "accent": 0, "muted": 0, "warn": 0, "good": 0, "bad": 0, "hot": 0}
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
    palette = {
        "base": curses.color_pair(1),
        "accent": curses.color_pair(6) | curses.A_BOLD,
        "muted": curses.color_pair(2),
        "warn": curses.color_pair(3) | curses.A_BOLD,
        "good": curses.color_pair(4) | curses.A_BOLD,
        "bad": curses.color_pair(5) | curses.A_BOLD,
        "hot": curses.color_pair(6) | curses.A_BOLD,
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


def _draw_header(stdscr: Any, workspace: str, mission_count: int, fleet_count: int, runtime: dict[str, Any], memory: dict[str, Any], health: dict[str, Any], colors: dict[str, int]) -> None:
    memory_counts = memory.get("counts", {})
    planner_backend = health.get("models", {}).get("planner", {}).get("backend", "heuristic")
    buddy_backend = health.get("models", {}).get("buddy", {}).get("backend", "heuristic")
    cockpit_state = "up" if runtime.get("fleet_session_exists") else "down"
    _safe_addstr(stdscr, 0, 2, "########## CONSTANT::DEMOSCENE::TUI ##########", colors["accent"])
    _safe_addstr(
        stdscr,
        1,
        2,
        _clip(
            f"workspace={workspace}  fleet={fleet_count}  cockpit={cockpit_state}  missions={mission_count}  docs={memory_counts.get('documents', 0)}  chunks={memory_counts.get('chunks', 0)}  decisions={memory_counts.get('decisions', 0)}",
            max(20, stdscr.getmaxyx()[1] - 4),
        ),
        colors["base"],
    )
    _safe_addstr(
        stdscr,
        2,
        2,
        _clip(
            f"planner={planner_backend}  buddy={buddy_backend}  keys: j/k mission  [/ ] machine  1..4 pane  o jump  r restart  z cockpit  q quit",
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
    _draw_box(stdscr, y, x, height, width, "Mission Deck", colors["accent"])
    row = y + 1
    if not missions:
        _safe_addstr(stdscr, row, x + 2, "No missions yet. Use `Constant mission create`.", colors["muted"])
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


def _draw_board(stdscr: Any, y: int, x: int, height: int, width: int, mission: dict[str, Any] | None, colors: dict[str, int]) -> None:
    _draw_box(stdscr, y, x, height, width, "Mission Board", colors["accent"])
    row = y + 1
    if not mission:
        lines = [
            "No active mission selected.",
            "",
            "Try:",
            "Constant mission create \"audit this repo\" --workspace \"$PWD\"",
        ]
        _write_wrapped(stdscr, row, x + 2, width - 4, lines, colors["muted"])
        return

    row = _write_wrapped(stdscr, row, x + 2, width - 4, [mission["title"]], colors["hot"])
    row += 1
    row = _write_wrapped(stdscr, row, x + 2, width - 4, [mission.get("goal", "")], colors["base"])
    row += 1
    _safe_addstr(stdscr, row, x + 2, f"workspace: {mission.get('workspace', '-')}", colors["muted"])
    row += 2
    _safe_addstr(stdscr, row, x + 2, "steps:", colors["accent"])
    row += 1
    for step in mission.get("steps", [])[: max(1, height - (row - y) - 2)]:
        line = f"[{_status_tag(step['status'])}] {step['step_id']}  {step['machine']}/{step['cli']}/{step['backend']}"
        _safe_addstr(stdscr, row, x + 2, _clip(line, width - 4), colors["base"])
        row += 1
        details = step.get("result_summary") or step.get("qwen_review") or step.get("title", "")
        if details and row < y + height - 1:
            row = _write_wrapped(stdscr, row, x + 4, width - 6, [details], colors["muted"])
        if row >= y + height - 1:
            break


def _draw_runtime(
    stdscr: Any,
    y: int,
    x: int,
    height: int,
    width: int,
    runtime: dict[str, Any],
    selected_machine: int,
    selected_role: str,
    colors: dict[str, int],
) -> None:
    _draw_box(stdscr, y, x, height, width, "Cockpit Runtime", colors["accent"])
    row = y + 1
    machines = runtime.get("machines", [])
    if not machines:
        _safe_addstr(stdscr, row, x + 2, "No runtime data yet. Open the cockpit with z.", colors["muted"])
        return

    header = "machine            claude codex copilot vibe  session  target"
    _safe_addstr(stdscr, row, x + 2, _clip(header, width - 4), colors["hot"])
    row += 1

    for index, machine in enumerate(machines[: max(1, height - 3)]):
        marker = ">" if index == selected_machine else " "
        attr = colors["accent"] if index == selected_machine else colors["base"]
        status = "up" if machine.get("session_exists") else "down"
        role_bits = []
        for role in ROLES:
            state = _role_state(machine, role)
            token = f"[{state}]"
            if role == selected_role and index == selected_machine:
                token = f"<{state}>"
            role_bits.append(token)
        line = f"{marker} {machine['label']:<16} {' '.join(role_bits)}  {status:<7} {machine['target']}"
        _safe_addstr(stdscr, row, x + 1, _clip(line, width - 3), attr)
        row += 1
        if row >= y + height - 1:
            break


def _draw_buddy(stdscr: Any, y: int, x: int, height: int, width: int, review: dict[str, str], memory: dict[str, Any], colors: dict[str, int]) -> None:
    _draw_box(stdscr, y, x, height, width, "Hexapus Buddy Rail", colors["accent"])
    row = y + 1
    verdict_attr = _verdict_attr(review, colors)
    for line in _hexapus_lines():
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
    }


def _run(stdscr: Any, workspace: str, local_session: str, machine_session: str) -> dict[str, Any] | None:
    try:
        curses.curs_set(0)
    except curses.error:
        pass
    stdscr.nodelay(False)
    stdscr.timeout(250)
    colors = _init_colors()
    selected_index = 0
    selected_machine = 0
    selected_role_index = 1
    flash = ""
    flash_until = 0.0
    runtime_cache: dict[str, Any] | None = None
    runtime_refresh_at = 0.0

    while True:
        now = time.time()
        if runtime_cache is None or now >= runtime_refresh_at:
            runtime_cache = _safe_runtime_status(local_session, machine_session)
            runtime_refresh_at = now + 2.0
        snapshot = _collect_snapshot(workspace, selected_index, local_session, machine_session, runtime_snapshot=runtime_cache)
        selected_index = snapshot["selected_index"]
        runtime_machines = snapshot["runtime"].get("machines", [])
        if runtime_machines:
            selected_machine = max(0, min(selected_machine, len(runtime_machines) - 1))
        else:
            selected_machine = 0
        stdscr.erase()
        height, width = stdscr.getmaxyx()

        _draw_header(
            stdscr,
            snapshot["workspace"],
            len(snapshot["missions"]),
            snapshot["fleet_count"],
            snapshot["runtime"],
            snapshot["memory"],
            snapshot["health"],
            colors,
        )

        top = 4
        timeline_height = min(10, max(7, height // 4))
        main_height = max(10, height - top - timeline_height - 1)
        left_width = max(28, min(34, width // 4))
        right_width = max(34, min(42, width // 3))
        center_width = max(28, width - left_width - right_width - 4)
        board_height = max(8, int(main_height * 0.52))
        runtime_height = max(6, main_height - board_height)

        _draw_missions(stdscr, top, 0, main_height, left_width, snapshot["missions"], selected_index, colors)
        _draw_board(stdscr, top, left_width + 1, board_height, center_width, snapshot["selected_mission"], colors)
        _draw_runtime(
            stdscr,
            top + board_height,
            left_width + 1,
            runtime_height,
            center_width,
            snapshot["runtime"],
            selected_machine,
            ROLES[selected_role_index],
            colors,
        )
        _draw_buddy(stdscr, top, left_width + center_width + 2, main_height, right_width, snapshot["review"], snapshot["memory"], colors)
        _draw_timeline(stdscr, top + main_height, 0, timeline_height, width, snapshot["events"], snapshot["persona_lines"], snapshot["decision_lines"], colors)

        if flash and time.time() < flash_until:
            _safe_addstr(stdscr, height - 1, 2, _clip(flash, width - 4), colors["warn"])
        else:
            flash = ""
            _safe_addstr(stdscr, height - 1, 2, "q quit | j/k mission | [/] machine | 1..4 pane | o jump | r restart | x capture | z open cockpit", colors["muted"])

        stdscr.refresh()
        key = stdscr.getch()
        if key == -1:
            continue
        if key in (ord("q"), 27):
            return None
        if key in (ord("j"), curses.KEY_DOWN):
            if snapshot["missions"]:
                selected_index = min(len(snapshot["missions"]) - 1, selected_index + 1)
            continue
        if key in (ord("k"), curses.KEY_UP):
            if snapshot["missions"]:
                selected_index = max(0, selected_index - 1)
            continue
        if key == ord("["):
            if snapshot["runtime"]["machines"]:
                selected_machine = max(0, selected_machine - 1)
            continue
        if key == ord("]"):
            if snapshot["runtime"]["machines"]:
                selected_machine = min(len(snapshot["runtime"]["machines"]) - 1, selected_machine + 1)
            continue
        if key in (ord("1"), ord("2"), ord("3"), ord("4")) and snapshot["runtime"]["machines"]:
            selected_role_index = int(chr(key)) - 1
            machine = snapshot["runtime"]["machines"][selected_machine]
            try:
                focus_machine(
                    machine["label"],
                    ROLES[selected_role_index],
                    local_session=local_session,
                    machine_session=machine_session,
                )
                runtime_cache = None
                flash = f"focused {machine['label']}:{ROLES[selected_role_index]}"
            except Exception as exc:  # noqa: BLE001
                flash = _error_line("focus failed", exc)
            flash_until = time.time() + 2.5
            continue
        if key == ord("o") and snapshot["runtime"]["machines"]:
            machine = snapshot["runtime"]["machines"][selected_machine]
            try:
                focus_machine(machine["label"], None, local_session=local_session, machine_session=machine_session)
                runtime_cache = None
                flash = f"jumped to {machine['label']}"
            except Exception as exc:  # noqa: BLE001
                flash = _error_line("jump failed", exc)
            flash_until = time.time() + 2.5
            continue
        if key == ord("r") and snapshot["runtime"]["machines"]:
            machine = snapshot["runtime"]["machines"][selected_machine]
            try:
                restart_pane(machine["label"], ROLES[selected_role_index], machine_session=machine_session)
                runtime_cache = None
                flash = f"restarted {machine['label']}:{ROLES[selected_role_index]}"
            except Exception as exc:  # noqa: BLE001
                flash = _error_line("restart failed", exc)
            flash_until = time.time() + 2.5
            continue
        if key == ord("x") and snapshot["runtime"]["machines"]:
            machine = snapshot["runtime"]["machines"][selected_machine]
            try:
                capture = capture_pane(machine["label"], ROLES[selected_role_index], lines=12, machine_session=machine_session)
                preview = (capture.get("stdout") or "").splitlines()[-1:] or ["(empty capture)"]
                flash = f"{machine['label']}:{ROLES[selected_role_index]} :: {preview[0]}"
            except Exception as exc:  # noqa: BLE001
                flash = _error_line("capture failed", exc)
            flash_until = time.time() + 3.5
            continue
        if key == ord("z"):
            return {"action": "cockpit", "workspace": snapshot["workspace"]}
        if key == ord("e"):
            try:
                rebuild_workspace_memory(snapshot["workspace"], enroll=True)
                flash = f"memory rebuilt for {snapshot['workspace']}"
            except Exception as exc:  # noqa: BLE001
                flash = _error_line("memory rebuild failed", exc)
            flash_until = time.time() + 2.5
            continue
        if key == ord("s") and snapshot["selected_mission"]:
            try:
                summarize_mission(snapshot["selected_mission"]["mission_id"])
                flash = f"mission summarized: {snapshot['selected_mission']['mission_id']}"
            except Exception as exc:  # noqa: BLE001
                flash = _error_line("summary failed", exc)
            flash_until = time.time() + 2.5
            continue


def run_tui(workspace: str, local_session: str = "constant-fleet", machine_session: str = "constant") -> dict[str, Any] | None:
    try:
        prime_workspace_memory(workspace, enroll=True)
    except Exception:
        pass
    return curses.wrapper(lambda stdscr: _run(stdscr, str(Path(workspace).expanduser().resolve()), local_session, machine_session))
