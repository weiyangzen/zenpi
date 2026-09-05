#!/usr/bin/env python3
"""Exercise installed TUI tool approval through a real terminal.

This is intentionally separate from ``user_smoke.py``.  It drives the binary
that the caller supplied, with no development fixtures, and uses a local
Responses-compatible HTTP server only as the model provider.  The two cases
cover both sides of the approval boundary: ``n`` must not write, while ``y``
must write and continue the provider turn.
"""

from __future__ import annotations

import argparse
import json
import os
import pty
import signal
import sys
import threading
import time
import fcntl
import struct
import termios
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from tempfile import TemporaryDirectory
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))

# Reuse the production smoke harness' bounded PTY and environment helpers.
from user_smoke import (  # noqa: E402
    drain_pty,
    isolated_env,
    json_lines,
    read_pty_until_exit,
    strip_ansi,
    terminate_pty_child,
)


class ResponsesFixture:
    """Small deterministic Responses SSE server for one interactive turn."""

    def __init__(self, continuation: str) -> None:
        self.requests: list[dict[str, Any]] = []
        self._lock = threading.Lock()
        self.continuation = continuation

        fixture = self

        class Handler(BaseHTTPRequestHandler):
            def do_POST(self) -> None:  # noqa: N802 - stdlib handler API
                try:
                    length = int(self.headers.get("content-length", "0"))
                    body = json.loads(self.rfile.read(length))
                except (ValueError, json.JSONDecodeError) as error:
                    self.send_error(400, f"invalid JSON: {error}")
                    return
                with fixture._lock:
                    fixture.requests.append(body)
                    request_number = len(fixture.requests)
                if self.path != "/v1/responses" or body.get("stream") is not True:
                    self.send_error(400, "expected streaming Responses request")
                    return
                if request_number == 1:
                    # The path is intentionally unique per run so a stale
                    # file cannot make a denied approval look successful.
                    payload = (
                        'data: {"type":"response.created","response":{"id":"tui-tool-1","model":"mock-model"}}\n\n'
                        'data: {"type":"response.output_item.done","item":{"type":"function_call","call_id":"tui-write-1","name":"write_file","arguments":"{\\"path\\":\\"decision.txt\\",\\"content\\":\\"approved by tui\\"}"}}\n\n'
                        'data: {"type":"response.completed","response":{"id":"tui-tool-1","model":"mock-model"}}\n\n'
                        "data: [DONE]\n"
                    )
                else:
                    payload = (
                        'data: {"type":"response.output_text.delta","delta":"'
                        + fixture.continuation
                        + '"}\n\n'
                        'data: {"type":"response.completed","response":{"id":"tui-tool-2","model":"mock-model"}}\n\n'
                        "data: [DONE]\n"
                    )
                response = payload.encode("utf-8")
                self.send_response(200)
                self.send_header("content-type", "text/event-stream")
                self.send_header("content-length", str(len(response)))
                self.send_header("connection", "close")
                self.end_headers()
                self.wfile.write(response)

            def log_message(self, *_args: Any) -> None:
                return

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.server.daemon_threads = True
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)

    def start(self) -> None:
        self.thread.start()

    def close(self) -> None:
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=3)
        if self.thread.is_alive():
            raise AssertionError("Responses fixture did not shut down")


def _send(fd: int, data: bytes) -> None:
    try:
        os.write(fd, data)
    except OSError as error:
        raise AssertionError(f"TUI PTY closed while sending {data!r}: {error}") from error


def _wait_for_text(
    fd: int,
    output: bytearray,
    needle: bytes,
    deadline: float,
) -> None:
    """Drain redraws like a real terminal until visible text appears."""
    while time.monotonic() < deadline:
        drain_pty(fd, output, min(0.05, max(0.0, deadline - time.monotonic())))
        if needle in strip_ansi(bytes(output)):
            return
    visible = strip_ansi(bytes(output))[-12000:]
    raise AssertionError(f"TUI did not render {needle!r}; tail={visible!r}")


def _wait_for_journal_turn(session: Path, role: str, deadline: float) -> None:
    while time.monotonic() < deadline:
        try:
            records = json_lines(session.read_text(encoding="utf-8"))
        except (FileNotFoundError, json.JSONDecodeError):
            records = []
        if any(record.get("kind") == "turn" and record.get("turn", {}).get("role") == role for record in records):
            return
        time.sleep(0.02)
    raise AssertionError(f"TUI did not persist a {role} turn")


def _wait_for_journal_content(session: Path, content: str, deadline: float) -> None:
    while time.monotonic() < deadline:
        try:
            records = json_lines(session.read_text(encoding="utf-8"))
        except (FileNotFoundError, json.JSONDecodeError):
            records = []
        if any(
            record.get("kind") == "turn"
            and content in record.get("turn", {}).get("content", "")
            for record in records
        ):
            return
        time.sleep(0.02)
    raise AssertionError(f"TUI did not persist provider continuation {content!r}")


def _wait_for_approval_receipt(session: Path, deadline: float) -> None:
    while time.monotonic() < deadline:
        try:
            records = json_lines(session.read_text(encoding="utf-8"))
        except (FileNotFoundError, json.JSONDecodeError):
            records = []
        if any(
            record.get("kind") == "event"
            and record.get("event", {}).get("type") == "approval_resolved"
            for record in records
        ):
            return
        time.sleep(0.02)
    raise AssertionError("TUI did not persist the approval decision")


def _run_case(binary: Path, root: Path, *, decision: bytes, continuation: str) -> None:
    allowed = decision == b"y"
    case = "allow" if allowed else "deny"
    workspace = root / case
    workspace.mkdir()
    session = workspace / "session.jsonl"
    home = root / f"{case}-home"
    fixture = ResponsesFixture(continuation)
    fixture.start()
    env = isolated_env(
        home,
        ZENPI_BASE_URL=f"http://127.0.0.1:{fixture.server.server_port}/v1",
        ZENPI_API_KEY="tui-approval-smoke-key",
        ZENPI_WIRE_API="responses",
        ZENPI_MODEL="mock-model",
    )
    command = [
        str(binary),
        "--mode",
        "tui",
        "--backend",
        "openai",
        "--model",
        "mock-model",
        "--session",
        str(session),
    ]
    pid, fd = pty.fork()
    output = bytearray()
    exited = False
    try:
        if pid == 0:
            os.chdir(workspace)
            os.execve(command[0], command, env)
        fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 140, 0, 0))
        _wait_for_text(fd, output, b"Prompt", time.monotonic() + 8)
        _send(fd, b"write the decision file\r")

        # Fold tool lifecycle entries while the model turn is in flight.  The
        # approval body arrives later as a normal system transcript message;
        # this ordering proves it is not hidden by the folded tool log.
        before_fold = len(output)
        _send(fd, b"\x0f")  # Ctrl-O: fold tool logs.
        fold_deadline = time.monotonic() + 3
        while time.monotonic() < fold_deadline:
            drain_pty(fd, output, 0.05)
            post_fold = strip_ansi(bytes(output[before_fold:]))
            # Captured PTY bytes are redraw deltas rather than a terminal
            # emulator's final screen: wide glyphs and spaces can be
            # overdrawn.  Compare a compact form, while still requiring the
            # approval and diff to be rendered after the fold toggle.
            compact = b"".join(post_fold.lower().split())
            if b"folded" in compact:
                break
        else:
            raise AssertionError(
                "TUI did not acknowledge folded tool logs; "
                f"tail={strip_ansi(bytes(output))[-12000:]!r}"
            )
        _wait_for_text(fd, output, b"Approval required", time.monotonic() + 15)
        visible = strip_ansi(bytes(output))
        normalized = bytes(character for character in visible.lower() if 48 <= character <= 57 or 97 <= character <= 122)
        if b"proposedchange" not in normalized or b"approvedbytui" not in normalized:
            raise AssertionError(
                "approval diff was not visible in the TUI transcript; "
                f"tail={visible[-12000:]!r}"
            )
        _send(fd, decision + b"\r")
        if allowed:
            # Allow returns a bounded tool result to the model. Waiting for
            # its durable text prevents Ctrl-D from interrupting continuation.
            _wait_for_journal_content(session, continuation, time.monotonic() + 15)
        else:
            # Denial is terminal by design: it is durable without another
            # provider request and must never create the requested file.
            _wait_for_approval_receipt(session, time.monotonic() + 10)
        _send(fd, b"\x04")  # deterministic empty-input quit
        exit_code, trailing = read_pty_until_exit(pid, fd, time.monotonic() + 10)
        exited = True
        output.extend(trailing)
        if exit_code != 0:
            raise AssertionError(
                f"installed TUI {case} case failed with {exit_code}; "
                f"tail={strip_ansi(bytes(output))[-12000:]!r}"
            )
    except BaseException:
        if not exited:
            terminate_pty_child(pid)
        raise
    finally:
        try:
            os.close(fd)
        except OSError:
            pass
        fixture.close()

    expected_requests = 2 if allowed else 1
    if len(fixture.requests) != expected_requests:
        raise AssertionError(
            f"{case} approval made {len(fixture.requests)} provider requests; "
            f"expected {expected_requests}"
        )
    if allowed:
        continuation_input = fixture.requests[1].get("input", [])
        if not any(item.get("type") == "function_call_output" for item in continuation_input):
            raise AssertionError(f"tool result was not sent to provider: {continuation_input!r}")
    target = workspace / "decision.txt"
    if not allowed:
        if target.exists():
            raise AssertionError("denied write_file changed the workspace")
    elif target.read_text(encoding="utf-8") != "approved by tui":
        raise AssertionError("allowed write_file did not change the workspace")
    records = json_lines(session.read_text(encoding="utf-8"))
    resolved = [
        record.get("event", {})
        for record in records
        if record.get("kind") == "event" and record.get("event", {}).get("type") == "approval_resolved"
    ]
    if not resolved:
        raise AssertionError(f"{case} approval result was not durable: {records!r}")
    expected = "allow" if allowed else "deny"
    if resolved[-1].get("decision") != expected:
        raise AssertionError(f"expected durable {expected} decision, got {resolved[-1]!r}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path, help="installed zenpi binary to exercise")
    args = parser.parse_args()
    binary = args.binary.expanduser().resolve()
    if not binary.is_file() or not os.access(binary, os.X_OK):
        parser.error(f"--binary is not executable: {binary}")
    with TemporaryDirectory(prefix="zenpi-tui-approval-") as temporary:
        root = Path(temporary)
        _run_case(
            binary,
            root,
            decision=b"n",
            continuation="write denied",
        )
        _run_case(
            binary,
            root,
            decision=b"y",
            continuation="write approved",
        )
    print("installed TUI approval smoke passed: folded approval diff, deny/no-write, allow/write, provider continuation, journal, and clean terminal exit")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
