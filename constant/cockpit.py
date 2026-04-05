from __future__ import annotations

import shlex
import subprocess
import socket
from pathlib import Path
from typing import Any

from .paths import scripts_dir
from .state import fleet_machine, load_fleet_config

ROLES = ("claude", "codex", "copilot", "vibe")


def _run(args: list[str]) -> dict[str, Any]:
    process = subprocess.run(args, capture_output=True, text=True)
    return {
        "argv": args,
        "returncode": process.returncode,
        "stdout": process.stdout,
        "stderr": process.stderr,
    }


def _tmux_list_command(session_name: str) -> list[str]:
    return [
        "tmux",
        "list-panes",
        "-t",
        session_name,
        "-F",
        "#{session_name}\t#{window_name}\t#{pane_id}\t#{pane_index}\t#{@constant_role}\t#{pane_title}\t#{pane_current_command}\t#{pane_active}\t#{pane_dead}\t#{pane_dead_status}",
    ]


def _ssh_command(target: str, inner: str) -> list[str]:
    remote_shell = (
        'PATH="$HOME/.local/bin:$HOME/.npm-global/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"; '
        "export PATH; "
        f"{inner}"
    )
    return ["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=1", target, remote_shell]


def _machine_session_name(session_name: str, machine_label: str) -> str:
    return f"{session_name}:{machine_label}"


def _parse_panes(stdout: str) -> list[dict[str, Any]]:
    panes: list[dict[str, Any]] = []
    for raw in stdout.splitlines():
        parts = raw.split("\t")
        if len(parts) != 10:
            continue
        session_name, window_name, pane_id, pane_index, pane_role, pane_title, pane_command, pane_active, pane_dead, pane_dead_status = parts
        role = pane_role or (pane_title if pane_title in ROLES else pane_command)
        panes.append(
            {
                "session_name": session_name,
                "window_name": window_name,
                "pane_id": pane_id,
                "pane_index": int(pane_index),
                "role": role,
                "pane_role": pane_role,
                "pane_title": pane_title,
                "pane_command": pane_command,
                "active": pane_active == "1",
                "dead": pane_dead == "1",
                "dead_status": int(pane_dead_status or "0"),
            }
        )
    return sorted(panes, key=lambda item: item["pane_index"])


def _machine_tmux_status(machine: dict[str, Any], session: str) -> dict[str, Any]:
    label = machine["label"]
    target = machine["target"]
    local_names = {socket.gethostname(), socket.getfqdn(), socket.gethostname().split(".")[0], "local", "localhost", "127.0.0.1", "::1"}
    is_local = target in local_names
    session_target = _machine_session_name(session, label)

    if is_local:
        result = _run(_tmux_list_command(session_target))
        if result["returncode"] != 0:
            for fallback_label in (socket.gethostname().split(".")[0], socket.getfqdn(), socket.gethostname()):
                fallback_target = _machine_session_name(session, fallback_label)
                result = _run(_tmux_list_command(fallback_target))
                if result["returncode"] == 0:
                    session_target = fallback_target
                    break
    else:
        inner = shlex.join(_tmux_list_command(session_target))
        result = _run(_ssh_command(target, inner))

    panes = _parse_panes(result["stdout"]) if result["returncode"] == 0 else []
    roles = {
        role: next((pane for pane in panes if pane["role"] == role), None)
        for role in ROLES
    }
    return {
        "label": label,
        "target": target,
        "session": session_target,
        "reachable": result["returncode"] == 0 or is_local,
        "attached_window": label,
        "session_exists": result["returncode"] == 0,
        "panes": panes,
        "roles": roles,
        "stderr": result["stderr"].strip(),
    }


def runtime_status(local_session: str = "constant-fleet", machine_session: str = "constant") -> dict[str, Any]:
    fleet = load_fleet_config()
    local_tmux = _run(["tmux", "list-windows", "-t", local_session, "-F", "#{window_name}"])
    fleet_windows = local_tmux["stdout"].splitlines() if local_tmux["returncode"] == 0 else []
    machines = [_machine_tmux_status(machine, machine_session) for machine in fleet["machines"]]
    return {
        "local_session": local_session,
        "machine_session": machine_session,
        "fleet_session_exists": local_tmux["returncode"] == 0,
        "fleet_windows": fleet_windows,
        "machines": machines,
        "fleet_stderr": local_tmux["stderr"].strip(),
    }


def cockpit_doctor(local_session: str = "constant-fleet", machine_session: str = "constant") -> dict[str, Any]:
    tmux_check = _run(["tmux", "-V"])
    payload = runtime_status(local_session=local_session, machine_session=machine_session)
    payload["tmux"] = {
        "available": tmux_check["returncode"] == 0,
        "stdout": tmux_check["stdout"].strip(),
        "stderr": tmux_check["stderr"].strip(),
    }
    return payload


def _machine_control_script(machine_label: str, machine_session: str) -> tuple[dict[str, Any], Path]:
    machine = fleet_machine(machine_label)
    script = scripts_dir() / "constant-machine.sh"
    return machine, script


def _run_machine_command(machine: dict[str, Any], args: list[str]) -> dict[str, Any]:
    target = machine["target"]
    local_names = {socket.gethostname(), socket.getfqdn(), socket.gethostname().split(".")[0], "local", "localhost", "127.0.0.1", "::1"}
    if target in local_names:
        return _run(args)

    quoted = shlex.join(args)
    return _run(_ssh_command(target, quoted))


def focus_machine(machine_label: str, pane_role: str | None, local_session: str = "constant-fleet", machine_session: str = "constant") -> dict[str, Any]:
    machine, script = _machine_control_script(machine_label, machine_session)
    if pane_role:
        payload = _run_machine_command(machine, [str(script), "--session", machine_session, "--focus-pane", pane_role])
        if payload["returncode"] != 0:
            return payload

    select_window = _run(["tmux", "select-window", "-t", f"{local_session}:{machine_label}"])
    return {
        "returncode": select_window["returncode"],
        "stdout": select_window["stdout"],
        "stderr": select_window["stderr"],
        "machine": machine_label,
        "pane": pane_role,
    }


def send_to_pane(machine_label: str, pane_role: str, command: str, machine_session: str = "constant") -> dict[str, Any]:
    machine, script = _machine_control_script(machine_label, machine_session)
    return _run_machine_command(
        machine,
        [str(script), "--session", machine_session, "--send-pane", pane_role, "--command", command],
    )


def capture_pane(machine_label: str, pane_role: str, lines: int = 120, machine_session: str = "constant") -> dict[str, Any]:
    machine, script = _machine_control_script(machine_label, machine_session)
    return _run_machine_command(
        machine,
        [str(script), "--session", machine_session, "--capture-pane", pane_role, "--lines", str(lines)],
    )


def restart_pane(machine_label: str, pane_role: str, machine_session: str = "constant") -> dict[str, Any]:
    machine, script = _machine_control_script(machine_label, machine_session)
    return _run_machine_command(
        machine,
        [str(script), "--session", machine_session, "--restart-pane", pane_role],
    )
