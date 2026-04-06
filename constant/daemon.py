from __future__ import annotations

import json
import os
import signal
import socket
import socketserver
import subprocess
import sys
import threading
import time
from pathlib import Path
from typing import Any

from .paths import daemon_log_path, daemon_pid_path, daemon_port_path
from .planner import PlannerEngine


_INLINE_ENGINE = PlannerEngine()


def _pid_is_alive(pid: int) -> bool:
    try:
        os.kill(pid, 0)
    except OSError:
        return False
    return True


def daemon_status() -> dict[str, Any]:
    pid_path = daemon_pid_path()
    port_path = daemon_port_path()
    pid = None
    running = False
    port = None

    if pid_path.exists():
        try:
            pid = int(pid_path.read_text(encoding="utf-8").strip())
            running = _pid_is_alive(pid)
        except ValueError:
            pid = None

    if port_path.exists():
        try:
            port = int(port_path.read_text(encoding="utf-8").strip())
        except ValueError:
            port = None

    endpoint = f"tcp://127.0.0.1:{port}" if port else None
    return {
        "running": running,
        "pid": pid,
        "port": port,
        "endpoint": endpoint,
        "log": str(daemon_log_path()),
    }


class _Handler(socketserver.StreamRequestHandler):
    engine = _INLINE_ENGINE

    def handle(self) -> None:
        raw = self.rfile.readline()
        if not raw:
            return

        try:
            request = json.loads(raw.decode("utf-8"))
            op = request["op"]
            if op == "health":
                payload = self.engine.health()
            elif op == "plan":
                payload = self.engine.plan_mission(request["mission"])
            elif op == "verify":
                payload = self.engine.verify_step(request["mission"], request["step"], request["execution"])
            elif op == "buddy":
                payload = self.engine.buddy_ask(request.get("mission"), request["prompt"])
            elif op == "chat":
                payload = self.engine.chat(
                    request["message"],
                    request.get("mission"),
                    request["workspace"],
                    request.get("selected_machine"),
                    request.get("selected_role"),
                    request.get("chat_history"),
                )
            else:
                raise KeyError(f"Unsupported operation: {op}")
            response = {"ok": True, "payload": payload}
        except Exception as exc:  # noqa: BLE001
            response = {"ok": False, "error": str(exc)}

        self.wfile.write((json.dumps(response) + "\n").encode("utf-8"))


class _TcpServer(socketserver.ThreadingMixIn, socketserver.TCPServer):
    daemon_threads = True
    allow_reuse_address = True


def _write_port(port: int) -> None:
    daemon_port_path().write_text(str(port), encoding="utf-8")


def _remove_port() -> None:
    port_path = daemon_port_path()
    if port_path.exists():
        port_path.unlink()


def serve_foreground() -> int:
    pid_path = daemon_pid_path()
    log_path = daemon_log_path()
    port_path = daemon_port_path()
    port_path.parent.mkdir(parents=True, exist_ok=True)
    log_path.parent.mkdir(parents=True, exist_ok=True)

    _remove_port()
    server = _TcpServer(("127.0.0.1", 0), _Handler)
    _write_port(int(server.server_address[1]))
    pid_path.write_text(str(os.getpid()), encoding="utf-8")

    def _shutdown(*_: object) -> None:
        thread = threading.Thread(target=server.shutdown, daemon=True)
        thread.start()

    signal.signal(signal.SIGTERM, _shutdown)
    signal.signal(signal.SIGINT, _shutdown)

    try:
        server.serve_forever()
    finally:
        server.server_close()
        _remove_port()
        if pid_path.exists():
            pid_path.unlink()

    return 0


def start_background() -> dict[str, Any]:
    status = daemon_status()
    if status["running"]:
        return status

    log_path = Path(status["log"])
    log_path.parent.mkdir(parents=True, exist_ok=True)

    with log_path.open("a", encoding="utf-8") as handle:
        process = subprocess.Popen(
            [sys.executable, "-m", "constant.cli", "__serve"],
            stdout=handle,
            stderr=handle,
            start_new_session=True,
        )

    for _ in range(50):
        status = daemon_status()
        if status["running"] and status["port"]:
            return status
        time.sleep(0.1)

    status = daemon_status()
    status["mode"] = "inline-fallback"
    return status


def stop_background() -> dict[str, Any]:
    status = daemon_status()
    if not status["running"] or not status["pid"]:
        return status

    os.kill(status["pid"], signal.SIGTERM)
    for _ in range(50):
        next_status = daemon_status()
        if not next_status["running"]:
            return next_status
        time.sleep(0.1)
    return daemon_status()


def _direct_request(op: str, payload: dict[str, Any] | None = None) -> dict[str, Any]:
    request_payload = payload or {}
    if op == "health":
        return _INLINE_ENGINE.health()
    if op == "plan":
        return _INLINE_ENGINE.plan_mission(request_payload["mission"])
    if op == "verify":
        return _INLINE_ENGINE.verify_step(
            request_payload["mission"],
            request_payload["step"],
            request_payload["execution"],
        )
    if op == "buddy":
        return _INLINE_ENGINE.buddy_ask(
            request_payload.get("mission"),
            request_payload["prompt"],
        )
    if op == "chat":
        return _INLINE_ENGINE.chat(
            request_payload["message"],
            request_payload.get("mission"),
            request_payload["workspace"],
            request_payload.get("selected_machine"),
            request_payload.get("selected_role"),
            request_payload.get("chat_history"),
        )
    raise RuntimeError(f"Unsupported operation: {op}")


def request(op: str, payload: dict[str, Any] | None = None, auto_start: bool = True) -> dict[str, Any]:
    if auto_start and not daemon_status()["running"]:
        start_background()

    status = daemon_status()
    if not status["running"] or not status["port"]:
        return _direct_request(op, payload)

    try:
        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        sock.connect(("127.0.0.1", int(status["port"])))
        with sock:
            message = {"op": op}
            if payload:
                message.update(payload)
            sock.sendall((json.dumps(message) + "\n").encode("utf-8"))
            raw = b""
            while not raw.endswith(b"\n"):
                chunk = sock.recv(65536)
                if not chunk:
                    break
                raw += chunk
    except OSError:
        return _direct_request(op, payload)

    response = json.loads(raw.decode("utf-8"))
    if not response.get("ok"):
        raise RuntimeError(response.get("error", "daemon request failed"))
    return response["payload"]
