#!/usr/bin/env python3
"""Production PTY smoke for the explicit TUI ``!`` user-shell lane.

This deliberately uses a fake OpenAI credential but never starts a provider
fixture: ``!echo`` must be handled locally and therefore cannot make an HTTP
request.  The smoke verifies the visible approval boundary, output rendering,
and persisted user-shell event without exercising an ordinary model turn.
"""

from __future__ import annotations

import argparse
import json
import os
import pty
import select
import signal
import struct
import sys
import termios
import time
import fcntl
from pathlib import Path
from tempfile import TemporaryDirectory


def drain(fd: int, out: bytearray, timeout: float = 0.1) -> None:
    ready, _, _ = select.select([fd], [], [], timeout)
    if not ready:
        return
    try:
        out.extend(os.read(fd, 65536))
    except OSError:
        pass


def visible(data: bytes) -> bytes:
    # Enough ANSI removal for redraw-oriented assertions; no terminal
    # emulator is needed for this smoke.
    import re

    return re.sub(rb"\x1b(?:\[[0-?]*[ -/]*[@-~]|\][^\a]*(?:\a|\x1b\\))", b"", data)


def wait_text(fd: int, out: bytearray, needle: bytes, deadline: float) -> None:
    while time.monotonic() < deadline:
        drain(fd, out, 0.05)
        if needle.lower() in visible(bytes(out)).lower():
            return
    raise AssertionError(f"missing {needle!r}; tail={visible(bytes(out))[-8000:]!r}")


def send(fd: int, payload: bytes) -> None:
    os.write(fd, payload)


def run_case(binary: Path, root: Path, *, command_text: bytes, decision: bytes, expected: bytes) -> list[dict]:
    session = root / ("allow.jsonl" if decision == b"y" else "deny.jsonl")
    home = root / ("allow-home" if decision == b"y" else "deny-home")
    env = {key: value for key, value in os.environ.items() if not key.startswith(("ZENPI_", "OPENAI_", "CODEX_"))}
    # Do not inherit a user's provider endpoint. A loopback port that is not
    # listening makes an accidental model submission both deterministic and
    # observable as a failure, while a local shell needs no provider at all.
    env.pop("ZENPI_BASE_URL", None)
    env.pop("OPENAI_BASE_URL", None)
    env.update(
        {
            "HOME": str(home),
            "ZENPI_HOME": str(home / ".zenpi"),
            "ZENPI_API_KEY": "pty-shell-smoke-key",
            "ZENPI_BASE_URL": "http://127.0.0.1:9/v1",
            "ZENPI_WIRE_API": "responses",
            "ZENPI_MODEL": "mock-model",
        }
    )
    argv = [
        str(binary), "--mode", "tui", "--backend", "openai", "--model", "mock-model",
        "--session", str(session),
    ]
    pid, fd = pty.fork()
    out = bytearray()
    if pid == 0:
        os.chdir(root)
        os.execve(argv[0], argv, env)
    try:
        fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 140, 0, 0))
        wait_text(fd, out, b"Prompt", time.monotonic() + 8)
        send(fd, command_text + b"\r")
        wait_text(fd, out, b"Approval required", time.monotonic() + 10)
        send(fd, decision + b"\r")
        wait_text(fd, out, expected, time.monotonic() + 12)
        send(fd, b"/quit\r")
        deadline = time.monotonic() + 8
        while time.monotonic() < deadline:
            drain(fd, out, 0.05)
            waited, status = os.waitpid(pid, os.WNOHANG)
            if waited == pid:
                if not os.WIFEXITED(status) or os.WEXITSTATUS(status) != 0:
                    raise AssertionError(f"TUI exited with status {status}")
                break
        else:
            raise AssertionError("TUI did not exit after /quit")
    finally:
        # Drain while stopping, then enforce a deadline even if terminal I/O
        # or a broken render loop cannot process the normal shutdown signal.
        try:
            if os.waitpid(pid, os.WNOHANG)[0] == 0:
                os.kill(pid, signal.SIGTERM)
                deadline = time.monotonic() + 1
                while time.monotonic() < deadline:
                    drain(fd, out, 0.05)
                    if os.waitpid(pid, os.WNOHANG)[0] == pid:
                        break
                else:
                    os.kill(pid, signal.SIGKILL)
                    os.waitpid(pid, 0)
        except (ChildProcessError, ProcessLookupError):
            pass
        os.close(fd)
    if not session.is_file():
        raise AssertionError("TUI shell smoke did not persist session")
    return [json.loads(line) for line in session.read_text(encoding="utf-8").splitlines() if line]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True, type=Path)
    args = parser.parse_args()
    args.binary = args.binary.resolve()
    with TemporaryDirectory(prefix="zenpi-tui-shell-") as raw:
        root = Path(raw)
        records = run_case(args.binary, root, command_text=b"!echo tui-shell-smoke", decision=b"y", expected=b"Local shell exit 0")
        records_deny = run_case(args.binary, root, command_text=b"!printf must-not-run > denied.txt", decision=b"n", expected=b"Request failed")
        shell_events = [
            record
            for record in records
            if record.get("kind") == "event"
            and record.get("event", {}).get("type")
            in {"user_shell_input", "tool_execution_finished"}
        ]
        shell_events.extend(
            record
            for record in records
            if record.get("kind") == "turn"
            and record.get("turn", {}).get("metadata", {}).get("origin") == "user_shell"
        )
        if not shell_events:
            raise AssertionError(f"no persisted user-shell event: {records!r}")
        turns = [record["turn"] for record in records if record.get("kind") == "turn"]
        if len(turns) != 1 or (turns[0].get("metadata") or {}).get("origin") != "user_shell":
            raise AssertionError(f"shell became a provider turn: {turns!r}")
        result = json.loads(turns[0]["content"].split("\n", 1)[1])
        if result["stdout"] != "tui-shell-smoke\n" or result["exit_code"] != 0 or not result["child_reaped"]:
            raise AssertionError(f"shell result missing actual output or reap evidence: {result!r}")
        if any(record.get("event", {}).get("operation_kind") == "provider" for record in records + records_deny):
            raise AssertionError("local shell unexpectedly started a provider turn")
        if (root / "denied.txt").exists():
            raise AssertionError("denied command changed a file")
        if any(
            record.get("kind") == "event"
            and record.get("event", {}).get("type") == "tool_execution_started"
            and record.get("event", {}).get("origin") == "user_shell"
            for record in records_deny
        ):
            raise AssertionError("denied command crossed the shell execution boundary")
    print("tui user-shell smoke passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
