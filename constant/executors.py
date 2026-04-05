from __future__ import annotations

import shlex
import subprocess
import time
from pathlib import Path
from typing import Any

from .paths import repo_root, scripts_dir
from .state import fleet_machine


def _run_command(args: list[str], cwd: str | None = None) -> dict[str, Any]:
    started = time.time()
    process = subprocess.run(args, capture_output=True, text=True, cwd=cwd)
    return {
        "argv": args,
        "returncode": process.returncode,
        "stdout": process.stdout,
        "stderr": process.stderr,
        "duration_s": round(time.time() - started, 3),
    }


def _local_agent_path_export() -> str:
    return 'export PATH="$HOME/.local/bin:$HOME/.npm-global/bin:$PATH";'


def _claude_command(prompt: str) -> list[str]:
    return ["claude", "-p", "--output-format", "json", "--permission-mode", "acceptEdits", prompt]


def _codex_command(prompt: str, workspace: str) -> list[str]:
    return ["codex", "exec", "--json", "--full-auto", "--skip-git-repo-check", "-C", workspace, prompt]


def _vibe_command(prompt: str, workspace: str) -> list[str]:
    return ["vibe", "-p", prompt, "--output", "json", "--workdir", workspace]


def _omc_command(cli: str, prompt: str) -> list[str]:
    if cli not in {"claude", "codex"}:
        raise RuntimeError(f"OMC backend only supports claude/codex in v1, not {cli}")
    return ["omc", "ask", cli, "--print", prompt]


def build_local_command(step: dict[str, Any], workspace: str) -> list[str]:
    cli = step["cli"]
    prompt = step["prompt"]
    backend = step["backend"]

    if backend == "omc":
        return _omc_command(cli, prompt)
    if cli == "claude":
        return _claude_command(prompt)
    if cli == "codex":
        return _codex_command(prompt, workspace)
    if cli == "vibe":
        return _vibe_command(prompt, workspace)
    raise RuntimeError(f"Unsupported auto CLI: {cli}")


def execute_step(step: dict[str, Any], mission: dict[str, Any], fleet: dict[str, Any]) -> dict[str, Any]:
    workspace = mission["workspace"]
    machine = fleet_machine(step["machine"])
    backend = step["backend"]

    if step["cli"] == "copilot":
        return {
            "argv": [],
            "returncode": 1,
            "stdout": "",
            "stderr": "copilot is manual-only in Constant v1",
            "duration_s": 0.0,
        }

    if backend in {"zellij", "cockpit"}:
        cockpit = str(scripts_dir() / "constant-fleet.sh")
        return {
            "argv": [cockpit, "--workspace", workspace],
            "returncode": 0,
            "stdout": f"Open cockpit manually with: {cockpit} --workspace {workspace}",
            "stderr": "",
            "duration_s": 0.0,
        }

    command = build_local_command(step, workspace)

    if backend == "cli-local" or backend == "omc":
        return _run_command(command, cwd=workspace)

    if backend == "cli-ssh":
        quoted = shlex.join(command)
        remote_shell = f'{_local_agent_path_export()} cd {shlex.quote(workspace)} && {quoted}'
        return _run_command(["ssh", machine["target"], "bash", "-lc", remote_shell])

    raise RuntimeError(f"Unsupported backend: {backend}")


def fleet_check() -> dict[str, Any]:
    check_script = repo_root() / "scripts" / "constant-fleet-install.sh"
    process = subprocess.run([str(check_script), "check"], capture_output=True, text=True)

    machines: list[dict[str, Any]] = []
    current: dict[str, Any] | None = None

    for raw_line in process.stdout.splitlines():
        line = raw_line.strip()
        if not line:
            continue
        if line.startswith("===") and "(" in line:
            if current:
                machines.append(current)
            label = line.split()[1]
            current = {"label": label}
            continue
        if "=" in line and current is not None:
            key, value = line.split("=", 1)
            current[key] = value

    if current:
        machines.append(current)

    return {
        "returncode": process.returncode,
        "machines": machines,
        "stderr": process.stderr,
    }


def bridge_sync() -> dict[str, Any]:
    bridge_script = repo_root() / "scripts" / "ai-bridge.sh"
    process = subprocess.run([str(bridge_script), "sync"], capture_output=True, text=True)
    return {
        "returncode": process.returncode,
        "stdout": process.stdout,
        "stderr": process.stderr,
    }
