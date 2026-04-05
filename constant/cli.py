from __future__ import annotations

import argparse
import json
import os
import sys
import time
from pathlib import Path
from typing import Any

from . import __version__
from .daemon import daemon_status, request as daemon_request, serve_foreground, start_background, stop_background
from .executors import bridge_sync, execute_step, fleet_check
from .memory import (
    enroll_workspace,
    list_decisions,
    memory_status,
    persona_markdown,
    rebuild_workspace_memory,
    search_memory,
    summarize_mission,
    sync_qdrant,
)
from .paths import cache_root, repo_root, scripts_dir
from .state import (
    append_event,
    create_mission,
    first_active_step,
    fleet_machine,
    load_fleet_config,
    load_mission,
    load_models_config,
    list_missions,
    mission_events_file,
    save_mission,
    write_artifact,
)
from .tui import run_tui


def _print(data: Any, as_json: bool = False) -> None:
    if as_json:
        print(json.dumps(data, indent=2, sort_keys=True))
    elif isinstance(data, str):
        print(data)
    else:
        print(json.dumps(data, indent=2, sort_keys=True))


def _command_exists(binary: str) -> bool:
    for path_dir in os.environ.get("PATH", "").split(":"):
        candidate = Path(path_dir) / binary
        if candidate.exists() and os.access(candidate, os.X_OK):
            return True
    return False


def _warm_memory(workspace: str) -> dict[str, Any] | None:
    try:
        return rebuild_workspace_memory(workspace, enroll=True)
    except Exception as exc:  # noqa: BLE001
        return {"error": str(exc), "workspace": workspace}


def _finalize_mission_memory(mission_id: str) -> dict[str, Any] | None:
    try:
        return summarize_mission(mission_id)
    except Exception:  # noqa: BLE001
        return None


def _ensure_planned(mission: dict[str, Any]) -> dict[str, Any]:
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


def _mission_summary(mission: dict[str, Any]) -> dict[str, Any]:
    return {
        "mission_id": mission["mission_id"],
        "title": mission["title"],
        "status": mission["status"],
        "workspace": mission["workspace"],
        "steps": [
            {
                "step_id": step["step_id"],
                "status": step["status"],
                "machine": step["machine"],
                "backend": step["backend"],
                "cli": step["cli"],
                "agent": step["agent"],
                "attempt": step["attempt"],
            }
            for step in mission["steps"]
        ],
    }


def cmd_doctor(args: argparse.Namespace) -> int:
    status = daemon_status()
    health = daemon_request("health", auto_start=False)
    fleet = load_fleet_config()
    models = load_models_config()
    report = {
        "version": __version__,
        "repo_root": str(repo_root()),
        "cache_root": str(cache_root()),
        "commands": {
            "python3": _command_exists("python3"),
            "tmux": _command_exists("tmux"),
            "omc": _command_exists("omc"),
            "claude": _command_exists("claude"),
            "codex": _command_exists("codex"),
            "copilot": _command_exists("copilot"),
            "vibe": _command_exists("vibe"),
            "constant-fleet": (scripts_dir() / "constant-fleet.sh").exists() or (scripts_dir() / "zellij-ai-fleet.sh").exists(),
            "ai-bridge": (scripts_dir() / "ai-bridge.sh").exists(),
        },
        "daemon": status,
        "models": models,
        "health": health,
        "memory": memory_status(),
        "fleet": [{"label": entry["label"], "target": entry["target"]} for entry in fleet["machines"]],
    }
    _print(report, args.json)
    return 0


def cmd_daemon_start(_: argparse.Namespace) -> int:
    payload = start_background()
    if not payload.get("running"):
        payload["mode"] = "inline-fallback"
    _print(payload)
    return 0


def cmd_daemon_stop(_: argparse.Namespace) -> int:
    _print(stop_background())
    return 0


def cmd_daemon_status(args: argparse.Namespace) -> int:
    payload = {"status": daemon_status(), "health": daemon_request("health", auto_start=False)}
    if not payload["status"]["running"]:
        payload["mode"] = "inline-fallback"
    _print(payload, args.json)
    return 0


def cmd_daemon_logs(_: argparse.Namespace) -> int:
    from .paths import daemon_log_path

    path = daemon_log_path()
    if not path.exists():
        print("No daemon log yet.")
        return 0
    print(path.read_text(encoding="utf-8"))
    return 0


def cmd_models_status(args: argparse.Namespace) -> int:
    payload = {"config": load_models_config(), "daemon": daemon_status(), "health": daemon_request("health", auto_start=False)}
    if not payload["daemon"]["running"]:
        payload["mode"] = "inline-fallback"
    _print(payload, args.json)
    return 0


def cmd_fleet_status(args: argparse.Namespace) -> int:
    payload = fleet_check()
    _print(payload, args.json)
    return 0 if payload["returncode"] == 0 else 1


def cmd_fleet_sync(args: argparse.Namespace) -> int:
    payload = bridge_sync()
    _print(payload, args.json)
    return 0 if payload["returncode"] == 0 else 1


def _exec_fleet_deploy_script(mode: str, args: argparse.Namespace) -> int:
    script = scripts_dir() / "constant-deploy.sh"
    command = [str(script), mode]

    for host in args.host or []:
        command.extend(["--host", host])
    if args.user:
        command.extend(["--user", args.user])
    if args.repo_dir:
        command.extend(["--repo-dir", args.repo_dir])
    if args.local_label:
        command.extend(["--local-label", args.local_label])
    if args.output:
        command.extend(["--output", args.output])
    if getattr(args, "json", False):
        command.append("--json")
    if getattr(args, "yes", False):
        command.append("--yes")
    if getattr(args, "all_reachable", False):
        command.append("--all-reachable")
    if getattr(args, "install", False):
        command.append("--install")
    if getattr(args, "no_ssh_config", False):
        command.append("--no-ssh-config")
    if getattr(args, "no_known_hosts", False):
        command.append("--no-known-hosts")
    if getattr(args, "no_arp", False):
        command.append("--no-arp")

    os.execv(command[0], command)
    return 0


def cmd_fleet_discover(args: argparse.Namespace) -> int:
    return _exec_fleet_deploy_script("scan", args)


def cmd_fleet_configure(args: argparse.Namespace) -> int:
    return _exec_fleet_deploy_script("configure", args)


def cmd_fleet_deploy(args: argparse.Namespace) -> int:
    return _exec_fleet_deploy_script("deploy", args)


def cmd_cockpit_open(args: argparse.Namespace) -> int:
    workspace = args.workspace or os.getcwd()
    command = [
        str((scripts_dir() / "constant-fleet.sh") if (scripts_dir() / "constant-fleet.sh").exists() else (scripts_dir() / "zellij-ai-fleet.sh")),
        "--workspace",
        workspace,
        "--local-session",
        args.local_session or "constant-fleet",
        "--session",
        args.session or "constant",
    ]
    os.execv(command[0], command)
    return 0


def cmd_tui(args: argparse.Namespace) -> int:
    workspace = str(Path(args.workspace or os.getcwd()).expanduser().resolve())
    action = run_tui(workspace)
    if action and action.get("action") == "cockpit":
        return cmd_cockpit_open(
            argparse.Namespace(
                workspace=action.get("workspace", workspace),
                local_session=args.local_session,
                session=args.session,
            )
        )
    return 0


def cmd_mission_create(args: argparse.Namespace) -> int:
    _warm_memory(args.workspace)
    mission = create_mission(args.prompt, args.workspace)
    mission = _ensure_planned(mission)
    _print(_mission_summary(mission), args.json)
    return 0


def cmd_mission_plan(args: argparse.Namespace) -> int:
    mission = load_mission(args.mission_id)
    _warm_memory(mission["workspace"])
    payload = daemon_request("plan", {"mission": mission})
    mission["steps"] = payload["plan"]["steps"]
    mission["title"] = payload["plan"]["title"]
    mission["status"] = "planned"
    mission["planner_summary"] = payload["plan"]["summary"]
    mission["buddy_review"] = payload["buddy_review"]
    save_mission(mission)
    append_event(mission["mission_id"], "mission.replanned", payload)
    _print({"summary": _mission_summary(mission), "buddy_review": payload["buddy_review"]}, args.json)
    return 0


def _verify_and_update(mission: dict[str, Any], step: dict[str, Any], execution: dict[str, Any]) -> dict[str, Any]:
    verdict = daemon_request("verify", {"mission": mission, "step": step, "execution": execution})
    step["verification"] = verdict
    step["result_summary"] = verdict.get("summary", "")
    return verdict


def cmd_mission_run(args: argparse.Namespace) -> int:
    mission = load_mission(args.mission_id)
    mission = _ensure_planned(mission)

    step = first_active_step(mission)
    if step is None:
        mission["status"] = "done"
        save_mission(mission)
        _print(_mission_summary(mission), args.json)
        return 0

    while step is not None:
        step["status"] = "running"
        step["attempt"] += 1
        mission["status"] = "running"
        save_mission(mission)
        append_event(mission["mission_id"], "step.started", {"step_id": step["step_id"], "machine": step["machine"], "backend": step["backend"], "cli": step["cli"]})

        execution = execute_step(step, mission, load_fleet_config())
        artifact_path = write_artifact(
            mission["mission_id"],
            f"{step['step_id']}-attempt-{step['attempt']}.json",
            execution,
        )
        step["artifact_refs"].append(artifact_path)
        append_event(mission["mission_id"], "step.executed", {"step_id": step["step_id"], "artifact": artifact_path, "returncode": execution["returncode"]})

        verdict = _verify_and_update(mission, step, execution)
        decision = verdict.get("decision", "failed")

        if decision == "done":
            step["status"] = "done"
        elif decision == "retry" and step["attempt"] < 2:
            step["status"] = "pending"
        elif decision == "needs_human":
            step["status"] = "needs_human"
            mission["status"] = "needs_human"
            save_mission(mission)
            append_event(mission["mission_id"], "step.needs_human", {"step_id": step["step_id"], "summary": verdict.get("summary", "")})
            _finalize_mission_memory(mission["mission_id"])
            _print({"summary": _mission_summary(mission), "verdict": verdict}, args.json)
            return 1
        else:
            step["status"] = "failed"
            mission["status"] = "failed"
            save_mission(mission)
            append_event(mission["mission_id"], "step.failed", {"step_id": step["step_id"], "summary": verdict.get("summary", "")})
            _finalize_mission_memory(mission["mission_id"])
            _print({"summary": _mission_summary(mission), "verdict": verdict}, args.json)
            return 1

        save_mission(mission)
        append_event(mission["mission_id"], "step.verified", {"step_id": step["step_id"], "verdict": verdict})
        step = first_active_step(mission)

    mission["status"] = "done"
    save_mission(mission)
    append_event(mission["mission_id"], "mission.done", {"mission_id": mission["mission_id"]})
    _finalize_mission_memory(mission["mission_id"])
    _print(_mission_summary(mission), args.json)
    return 0


def cmd_mission_status(args: argparse.Namespace) -> int:
    if args.mission_id:
        mission = load_mission(args.mission_id)
        _print(_mission_summary(mission) if not args.verbose else mission, args.json)
        return 0

    missions = [_mission_summary(mission) for mission in list_missions()]
    _print({"missions": missions}, args.json)
    return 0


def cmd_mission_tail(args: argparse.Namespace) -> int:
    path = mission_events_file(args.mission_id)
    if not path.exists():
        print(f"No events for mission {args.mission_id}")
        return 1

    if not args.follow:
        print(path.read_text(encoding="utf-8"), end="")
        return 0

    seen_size = 0
    while True:
        text = path.read_text(encoding="utf-8")
        if len(text) > seen_size:
            print(text[seen_size:], end="")
            seen_size = len(text)
        time.sleep(1)


def cmd_mission_verify(args: argparse.Namespace) -> int:
    mission = load_mission(args.mission_id)
    step = next((entry for entry in mission["steps"] if entry["step_id"] == args.step_id), None) if args.step_id else (mission["steps"][-1] if mission["steps"] else None)
    if step is None or not step["artifact_refs"]:
        print("No step artifact available to verify.")
        return 1

    artifact = json.loads(Path(step["artifact_refs"][-1]).read_text(encoding="utf-8"))
    verdict = _verify_and_update(mission, step, artifact)
    save_mission(mission)
    append_event(mission["mission_id"], "mission.verify", {"step_id": step["step_id"], "verdict": verdict})
    _print({"step_id": step["step_id"], "verdict": verdict}, args.json)
    return 0 if verdict.get("decision") == "done" else 1


def cmd_mission_retry(args: argparse.Namespace) -> int:
    mission = load_mission(args.mission_id)
    step = next((entry for entry in mission["steps"] if entry["step_id"] == args.step_id), None) if args.step_id else first_active_step(mission) or (mission["steps"][-1] if mission["steps"] else None)
    if step is None:
        print("No step to retry.")
        return 1

    step["status"] = "pending"
    mission["status"] = "planned"
    save_mission(mission)
    append_event(mission["mission_id"], "step.retry_requested", {"step_id": step["step_id"]})
    _print(_mission_summary(mission), args.json)
    return 0


def cmd_delegate(args: argparse.Namespace) -> int:
    mission = load_mission(args.mission_id)
    step = next((entry for entry in mission["steps"] if entry["step_id"] == args.step_id), None) if args.step_id else first_active_step(mission)
    if step is None:
        print("No active step to delegate.")
        return 1

    if args.machine:
        fleet_machine(args.machine)
        step["machine"] = args.machine
    if args.backend:
        step["backend"] = args.backend
    if args.cli:
        step["cli"] = args.cli
    if args.agent:
        step["agent"] = args.agent
    step["status"] = "pending"
    mission["status"] = "planned"
    save_mission(mission)
    append_event(mission["mission_id"], "step.delegated", {"step_id": step["step_id"], "machine": step["machine"], "backend": step["backend"], "cli": step["cli"], "agent": step["agent"]})
    _print(_mission_summary(mission), args.json)
    return 0


def cmd_buddy_ask(args: argparse.Namespace) -> int:
    mission = load_mission(args.mission_id) if args.mission_id else None
    if mission:
        _warm_memory(mission["workspace"])
    answer = daemon_request("buddy", {"mission": mission, "prompt": args.prompt})
    if mission:
        append_event(mission["mission_id"], "buddy.ask", {"prompt": args.prompt, "answer": answer})
    _print(answer, args.json)
    return 0


def cmd_memory_status(args: argparse.Namespace) -> int:
    _print(memory_status(args.workspace), args.json)
    return 0


def cmd_memory_rebuild(args: argparse.Namespace) -> int:
    _print(rebuild_workspace_memory(args.workspace, enroll=not args.no_enroll), args.json)
    return 0


def cmd_memory_enroll(args: argparse.Namespace) -> int:
    _print(enroll_workspace(args.path), args.json)
    return 0


def cmd_memory_search(args: argparse.Namespace) -> int:
    _print(search_memory(args.query, args.workspace, args.limit), args.json)
    return 0


def cmd_memory_persona_show(args: argparse.Namespace) -> int:
    payload = {"persona": persona_markdown()} if args.json else persona_markdown()
    _print(payload, args.json)
    return 0


def cmd_memory_decisions(args: argparse.Namespace) -> int:
    _print(list_decisions(args.workspace, args.mission_id), args.json)
    return 0


def cmd_memory_sync_qdrant(args: argparse.Namespace) -> int:
    payload = sync_qdrant(args.workspace)
    _print(payload, args.json)
    return 0 if payload.get("ok") or payload.get("skipped") else 1


def cmd_mission_summarize(args: argparse.Namespace) -> int:
    _print(summarize_mission(args.mission_id), args.json)
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="Constant")
    parser.add_argument("-V", "--version", action="version", version=__version__)
    subparsers = parser.add_subparsers(dest="command", required=True)

    doctor = subparsers.add_parser("doctor")
    doctor.add_argument("--json", action="store_true")
    doctor.set_defaults(func=cmd_doctor)

    tui = subparsers.add_parser("tui")
    tui.add_argument("--workspace")
    tui.add_argument("--local-session")
    tui.add_argument("--session")
    tui.set_defaults(func=cmd_tui)

    daemon = subparsers.add_parser("daemon")
    daemon_sub = daemon.add_subparsers(dest="daemon_command", required=True)
    daemon_start = daemon_sub.add_parser("start")
    daemon_start.set_defaults(func=cmd_daemon_start)
    daemon_stop = daemon_sub.add_parser("stop")
    daemon_stop.set_defaults(func=cmd_daemon_stop)
    daemon_status_cmd = daemon_sub.add_parser("status")
    daemon_status_cmd.add_argument("--json", action="store_true")
    daemon_status_cmd.set_defaults(func=cmd_daemon_status)
    daemon_logs = daemon_sub.add_parser("logs")
    daemon_logs.set_defaults(func=cmd_daemon_logs)

    models = subparsers.add_parser("models")
    models_sub = models.add_subparsers(dest="models_command", required=True)
    models_status = models_sub.add_parser("status")
    models_status.add_argument("--json", action="store_true")
    models_status.set_defaults(func=cmd_models_status)

    fleet = subparsers.add_parser("fleet")
    fleet_sub = fleet.add_subparsers(dest="fleet_command", required=True)
    fleet_status_cmd = fleet_sub.add_parser("status")
    fleet_status_cmd.add_argument("--json", action="store_true")
    fleet_status_cmd.set_defaults(func=cmd_fleet_status)
    fleet_sync_cmd = fleet_sub.add_parser("sync")
    fleet_sync_cmd.add_argument("--json", action="store_true")
    fleet_sync_cmd.set_defaults(func=cmd_fleet_sync)
    for fleet_cmd_name, fleet_cmd_func, allow_json in (
        ("discover", cmd_fleet_discover, True),
        ("configure", cmd_fleet_configure, False),
        ("deploy", cmd_fleet_deploy, False),
    ):
        fleet_cmd = fleet_sub.add_parser(fleet_cmd_name)
        fleet_cmd.add_argument("--host", action="append")
        fleet_cmd.add_argument("--user")
        fleet_cmd.add_argument("--repo-dir")
        fleet_cmd.add_argument("--local-label")
        fleet_cmd.add_argument("--output")
        fleet_cmd.add_argument("--yes", action="store_true")
        fleet_cmd.add_argument("--all-reachable", action="store_true")
        fleet_cmd.add_argument("--no-ssh-config", action="store_true")
        fleet_cmd.add_argument("--no-known-hosts", action="store_true")
        fleet_cmd.add_argument("--no-arp", action="store_true")
        if fleet_cmd_name == "configure":
            fleet_cmd.add_argument("--install", action="store_true")
        if allow_json:
            fleet_cmd.add_argument("--json", action="store_true")
        fleet_cmd.set_defaults(func=fleet_cmd_func)

    cockpit = subparsers.add_parser("cockpit")
    cockpit_sub = cockpit.add_subparsers(dest="cockpit_command", required=True)
    cockpit_open = cockpit_sub.add_parser("open")
    cockpit_open.add_argument("--workspace")
    cockpit_open.add_argument("--local-session")
    cockpit_open.add_argument("--session")
    cockpit_open.set_defaults(func=cmd_cockpit_open)

    mission = subparsers.add_parser("mission")
    mission_sub = mission.add_subparsers(dest="mission_command", required=True)
    mission_create = mission_sub.add_parser("create")
    mission_create.add_argument("prompt")
    mission_create.add_argument("--workspace", default=os.getcwd())
    mission_create.add_argument("--json", action="store_true")
    mission_create.set_defaults(func=cmd_mission_create)
    mission_plan = mission_sub.add_parser("plan")
    mission_plan.add_argument("mission_id")
    mission_plan.add_argument("--json", action="store_true")
    mission_plan.set_defaults(func=cmd_mission_plan)
    mission_run = mission_sub.add_parser("run")
    mission_run.add_argument("mission_id")
    mission_run.add_argument("--json", action="store_true")
    mission_run.set_defaults(func=cmd_mission_run)
    mission_status = mission_sub.add_parser("status")
    mission_status.add_argument("mission_id", nargs="?")
    mission_status.add_argument("--verbose", action="store_true")
    mission_status.add_argument("--json", action="store_true")
    mission_status.set_defaults(func=cmd_mission_status)
    mission_tail = mission_sub.add_parser("tail")
    mission_tail.add_argument("mission_id")
    mission_tail.add_argument("--follow", action="store_true")
    mission_tail.set_defaults(func=cmd_mission_tail)
    mission_verify = mission_sub.add_parser("verify")
    mission_verify.add_argument("mission_id")
    mission_verify.add_argument("--step-id")
    mission_verify.add_argument("--json", action="store_true")
    mission_verify.set_defaults(func=cmd_mission_verify)
    mission_retry = mission_sub.add_parser("retry")
    mission_retry.add_argument("mission_id")
    mission_retry.add_argument("--step-id")
    mission_retry.add_argument("--json", action="store_true")
    mission_retry.set_defaults(func=cmd_mission_retry)
    mission_summarize = mission_sub.add_parser("summarize")
    mission_summarize.add_argument("mission_id")
    mission_summarize.add_argument("--json", action="store_true")
    mission_summarize.set_defaults(func=cmd_mission_summarize)

    delegate = subparsers.add_parser("delegate")
    delegate.add_argument("mission_id")
    delegate.add_argument("--step-id")
    delegate.add_argument("--machine")
    delegate.add_argument("--backend")
    delegate.add_argument("--cli")
    delegate.add_argument("--agent")
    delegate.add_argument("--json", action="store_true")
    delegate.set_defaults(func=cmd_delegate)

    buddy = subparsers.add_parser("buddy")
    buddy_sub = buddy.add_subparsers(dest="buddy_command", required=True)
    buddy_ask = buddy_sub.add_parser("ask")
    buddy_ask.add_argument("prompt")
    buddy_ask.add_argument("--mission-id")
    buddy_ask.add_argument("--json", action="store_true")
    buddy_ask.set_defaults(func=cmd_buddy_ask)

    memory = subparsers.add_parser("memory")
    memory_sub = memory.add_subparsers(dest="memory_command", required=True)
    memory_status_cmd = memory_sub.add_parser("status")
    memory_status_cmd.add_argument("--workspace")
    memory_status_cmd.add_argument("--json", action="store_true")
    memory_status_cmd.set_defaults(func=cmd_memory_status)
    memory_rebuild_cmd = memory_sub.add_parser("rebuild")
    memory_rebuild_cmd.add_argument("--workspace", default=os.getcwd())
    memory_rebuild_cmd.add_argument("--no-enroll", action="store_true")
    memory_rebuild_cmd.add_argument("--json", action="store_true")
    memory_rebuild_cmd.set_defaults(func=cmd_memory_rebuild)
    memory_enroll_cmd = memory_sub.add_parser("enroll")
    memory_enroll_cmd.add_argument("path")
    memory_enroll_cmd.add_argument("--json", action="store_true")
    memory_enroll_cmd.set_defaults(func=cmd_memory_enroll)
    memory_search_cmd = memory_sub.add_parser("search")
    memory_search_cmd.add_argument("query")
    memory_search_cmd.add_argument("--workspace")
    memory_search_cmd.add_argument("--limit", type=int)
    memory_search_cmd.add_argument("--json", action="store_true")
    memory_search_cmd.set_defaults(func=cmd_memory_search)
    memory_persona_cmd = memory_sub.add_parser("persona")
    memory_persona_sub = memory_persona_cmd.add_subparsers(dest="memory_persona_command", required=True)
    memory_persona_show_cmd = memory_persona_sub.add_parser("show")
    memory_persona_show_cmd.add_argument("--json", action="store_true")
    memory_persona_show_cmd.set_defaults(func=cmd_memory_persona_show)
    memory_decisions_cmd = memory_sub.add_parser("decisions")
    memory_decisions_cmd.add_argument("--workspace")
    memory_decisions_cmd.add_argument("--mission-id")
    memory_decisions_cmd.add_argument("--json", action="store_true")
    memory_decisions_cmd.set_defaults(func=cmd_memory_decisions)
    memory_sync_cmd = memory_sub.add_parser("sync-qdrant")
    memory_sync_cmd.add_argument("--workspace")
    memory_sync_cmd.add_argument("--json", action="store_true")
    memory_sync_cmd.set_defaults(func=cmd_memory_sync_qdrant)

    hidden = subparsers.add_parser("__serve")
    hidden.set_defaults(func=lambda _args: serve_foreground())

    return parser


def main(argv: list[str] | None = None) -> int:
    if argv is None:
        argv = sys.argv[1:]
    if not argv:
        if sys.stdin.isatty() and sys.stdout.isatty():
            argv = ["tui", "--workspace", os.getcwd()]
        else:
            argv = ["doctor"]
    parser = build_parser()
    args = parser.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
