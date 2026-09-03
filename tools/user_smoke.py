#!/usr/bin/env python3
"""Exercise zenpi as an installed release artifact, not just a Cargo target."""

from __future__ import annotations

import json
import os
import pty
import re
import select
import signal
import struct
import subprocess
import tempfile
import termios
import threading
import time
import fcntl
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]


def run(command: list[str], *, input_text: str = "", env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        command,
        cwd=ROOT,
        input=input_text,
        text=True,
        capture_output=True,
        timeout=45,
        env=env,
        check=False,
    )


def json_lines(text: str) -> list[dict[str, Any]]:
    records = []
    for line_number, line in enumerate(text.splitlines(), 1):
        if not line.strip():
            continue
        value = json.loads(line)
        if not isinstance(value, dict):
            raise AssertionError(f"line {line_number} is not an object")
        records.append(value)
    return records


def assert_success(result: subprocess.CompletedProcess[str], context: str) -> None:
    if result.returncode != 0:
        raise AssertionError(
            f"{context} failed with {result.returncode}:\n"
            f"stdout={result.stdout}\nstderr={result.stderr}"
        )


def assert_headless_echo(binary: Path, root: Path) -> Path:
    session = root / "echo-session.jsonl"
    payload = (
        '{"type":"prompt","id":"p","text":"installed echo works"}\n'
        '{"type":"status","id":"s"}\n'
        '{"type":"shutdown","id":"q"}\n'
    )
    result = run(
        [str(binary), "--mode", "headless", "--backend", "echo", "--session", str(session)],
        input_text=payload,
    )
    assert_success(result, "installed headless echo")
    records = json_lines(result.stdout)
    responses = {record["id"]: record for record in records if record.get("type") == "response"}
    for request_id in ("p", "s", "q"):
        if request_id not in responses or responses[request_id].get("success") is not True:
            raise AssertionError(f"missing successful response for {request_id}: {records!r}")
    assistant = responses["p"].get("data", {}).get("assistant", {})
    if assistant.get("content") != "installed echo works":
        raise AssertionError(f"echo response mismatch: {assistant!r}")
    if not session.is_file():
        raise AssertionError("echo session was not persisted")
    journal = json_lines(session.read_text(encoding="utf-8"))
    if sum(record.get("kind") == "turn" for record in journal) != 2:
        raise AssertionError(f"echo session does not contain user/assistant turns: {journal!r}")
    return session


def assert_resume(binary: Path, session: Path, root: Path) -> None:
    resumed = root / "resumed-session.jsonl"
    payload = (
        f'{{"type":"resume","id":"r","path":{json.dumps(str(session))}}}\n'
        '{"type":"status","id":"s"}\n'
        '{"type":"shutdown","id":"q"}\n'
    )
    result = run(
        [str(binary), "--mode", "headless", "--backend", "echo", "--session", str(resumed)],
        input_text=payload,
    )
    assert_success(result, "installed resume/status")
    responses = {
        record["id"]: record
        for record in json_lines(result.stdout)
        if record.get("type") == "response" and record.get("id")
    }
    summary = responses.get("s", {}).get("data", {}).get("session", {})
    if summary.get("turn_count") != 2:
        raise AssertionError(f"resume did not recover two turns: {summary!r}")


def assert_resume_reopens_process(binary: Path, session: Path, root: Path) -> None:
    """Prove resume reads a journal written by a prior process."""
    fresh = root / "fresh-process.jsonl"
    first = run(
        [str(binary), "--mode", "headless", "--backend", "echo", "--session", str(fresh)],
        input_text=(
            '{"type":"prompt","id":"first","text":"persist across process"}\n'
            '{"type":"shutdown","id":"done"}\n'
        ),
    )
    assert_success(first, "first process before resume")
    second = run(
        [str(binary), "--mode", "headless", "--backend", "echo", "--session", str(root / "second.jsonl")],
        input_text=(
            f'{{"type":"resume","id":"resume","path":{json.dumps(str(fresh))}}}\n'
            '{"type":"status","id":"status"}\n'
            '{"type":"shutdown","id":"done"}\n'
        ),
    )
    assert_success(second, "second process resume")
    responses = {
        record["id"]: record
        for record in json_lines(second.stdout)
        if record.get("type") == "response" and record.get("id")
    }
    if responses.get("status", {}).get("data", {}).get("session", {}).get("turn_count") != 2:
        raise AssertionError(f"second process did not recover turns: {responses!r}")


def assert_invalid_inputs(binary: Path, root: Path) -> None:
    missing_key_session = root / "missing-key.jsonl"
    env = {key: value for key, value in os.environ.items() if key not in {"ZENPI_API_KEY", "OPENAI_API_KEY"}}
    # Deliberately omit the path: the binary must normalize the host before
    # applying its credential guard.
    env["ZENPI_BASE_URL"] = "https://api.openai.com"
    result = run(
        [str(binary), "--mode", "headless", "--backend", "openai", "--session", str(missing_key_session)],
        env=env,
    )
    if result.returncode == 0 or missing_key_session.exists() or "API_KEY" not in result.stderr:
        raise AssertionError("missing OpenAI credentials did not fail before session creation")

    directory = root / "session-directory"
    directory.mkdir()
    result = run(
        [str(binary), "--mode", "headless", "--backend", "echo", "--session", str(directory)],
    )
    if result.returncode == 0 or "directory" not in result.stderr:
        raise AssertionError("directory session path was accepted")


class _MockHandler(BaseHTTPRequestHandler):
    request: dict[str, Any] = {}

    def do_POST(self) -> None:  # noqa: N802 - stdlib handler API
        length = int(self.headers.get("content-length", "0"))
        body = json.loads(self.rfile.read(length))
        _MockHandler.request = {
            "path": self.path,
            "model": body.get("model"),
            "stream": body.get("stream"),
            "authorization": self.headers.get("authorization"),
            "messages": body.get("messages"),
        }
        response = json.dumps(
            {
                "id": "chatcmpl-user-smoke",
                "model": "mock-model",
                "choices": [{"message": {"role": "assistant", "content": "provider works"}}],
                "usage": {"prompt_tokens": 3, "completion_tokens": 2, "total_tokens": 5},
            }
        ).encode("utf-8")
        self.send_response(200)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(response)))
        self.end_headers()
        self.wfile.write(response)

    def log_message(self, *_args: Any) -> None:
        return


def assert_openai_fixture(binary: Path, root: Path) -> None:
    _MockHandler.request = {}
    server = ThreadingHTTPServer(("127.0.0.1", 0), _MockHandler)
    server.timeout = 10
    server_thread = threading.Thread(target=server.handle_request, daemon=True)
    server_thread.start()
    session = root / "provider-session.jsonl"
    env = {**os.environ, "ZENPI_BASE_URL": f"http://127.0.0.1:{server.server_port}/v1", "ZENPI_API_KEY": "user-smoke-key"}
    payload = (
        '{"type":"prompt","id":"p","text":"provider prompt"}\n'
        '{"type":"shutdown","id":"q"}\n'
    )
    result = run(
        [str(binary), "--mode", "headless", "--backend", "openai", "--model", "mock-model", "--session", str(session)],
        input_text=payload,
        env=env,
    )
    server_thread.join(timeout=12)
    server.server_close()
    if server_thread.is_alive():
        raise AssertionError("OpenAI fixture did not receive the request")
    assert_success(result, "OpenAI-compatible fixture")
    request = _MockHandler.request
    if request.get("path") != "/v1/chat/completions":
        raise AssertionError(f"unexpected provider path: {request!r}")
    if request.get("model") != "mock-model" or request.get("stream") is not False:
        raise AssertionError(f"provider request contract mismatch: {request!r}")
    if request.get("authorization") != "Bearer user-smoke-key":
        raise AssertionError("provider authorization header was not sent")
    responses = {
        record["id"]: record
        for record in json_lines(result.stdout)
        if record.get("type") == "response" and record.get("id")
    }
    if responses.get("p", {}).get("data", {}).get("assistant", {}).get("content") != "provider works":
        raise AssertionError(f"provider response was not surfaced: {responses!r}")


def read_pty_until_exit(pid: int, fd: int, deadline: float) -> tuple[int, bytes]:
    output = bytearray()
    status: int | None = None
    while time.monotonic() < deadline:
        waited, wait_status = os.waitpid(pid, os.WNOHANG)
        if waited == pid:
            status = wait_status
            break
        ready, _, _ = select.select([fd], [], [], 0.1)
        if ready:
            try:
                output.extend(os.read(fd, 65536))
            except OSError:
                # Linux PTYs report EIO after the child closes the slave. The
                # child may already be a zombie, so poll waitpid once more
                # before treating this as a timeout.
                waited, wait_status = os.waitpid(pid, os.WNOHANG)
                if waited == pid:
                    status = wait_status
                    break
    if status is None:
        os.kill(pid, signal.SIGTERM)
        _, status = os.waitpid(pid, 0)
        raise AssertionError(f"TUI did not exit before deadline; output={bytes(output)!r}")
    for _ in range(3):
        ready, _, _ = select.select([fd], [], [], 0.1)
        if not ready:
            break
        try:
            output.extend(os.read(fd, 65536))
        except OSError:
            break
    return os.waitstatus_to_exitcode(status), bytes(output)


def wait_for_tui_turn(session: Path, timeout: float = 10) -> None:
    """Wait for the synchronous echo turn to be durable before quitting."""
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            records = json_lines(session.read_text(encoding="utf-8"))
        except (FileNotFoundError, json.JSONDecodeError):
            records = []
        if any(
            record.get("kind") == "turn"
            and record.get("turn", {}).get("role") == "assistant"
            for record in records
        ):
            return
        time.sleep(0.05)
    raise AssertionError("TUI did not persist the submitted turn before exit")


def strip_ansi(data: bytes) -> bytes:
    """Remove terminal control sequences before inspecting PTY text."""
    return re.sub(rb"\x1b\[[0-?]*[ -/]*[@-~]", b"", data).replace(b"\r", b"")


def assert_tui(binary: Path, root: Path) -> None:
    session = root / "tui-session.jsonl"
    command = [str(binary), "--mode", "tui", "--backend", "echo", "--session", str(session)]
    pid, fd = pty.fork()
    if pid == 0:
        os.execv(command[0], command)
    exited = False
    try:
        # A real terminal supplies a usable window size before the first draw.
        fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 100, 0, 0))
        output = bytearray()
        ready_deadline = time.monotonic() + 5
        while time.monotonic() < ready_deadline and b"Prompt" not in output:
            ready, _, _ = select.select([fd], [], [], 0.1)
            if ready:
                try:
                    output.extend(os.read(fd, 65536))
                except OSError:
                    break
        if b"Prompt" not in output:
            raise AssertionError(f"TUI did not reach prompt: {bytes(output)!r}")
        os.write(fd, b"hello from installed tui")
        os.write(fd, b"\r")
        # Enter synchronously completes the deterministic echo turn. Wait for
        # its journal record so a slow CI host cannot race Ctrl-C with Enter.
        wait_for_tui_turn(session)
        os.write(fd, b"\x03")
        # Some Linux PTY setups deliver Ctrl-C as a signal rather than as a
        # crossterm key event. Ctrl-D is the TUI's empty-input quit binding and
        # makes the cleanup assertion deterministic on both PTY variants.
        time.sleep(0.25)
        try:
            os.write(fd, b"\x04")
        except OSError:
            pass
        exit_code, trailing = read_pty_until_exit(pid, fd, time.monotonic() + 10)
        exited = True
        output.extend(trailing)
    except BaseException:
        if not exited:
            try:
                os.kill(pid, signal.SIGTERM)
                os.waitpid(pid, 0)
            except (OSError, ChildProcessError):
                pass
        raise
    finally:
        try:
            os.close(fd)
        except OSError:
            pass
    if exit_code != 0:
        raise AssertionError(f"TUI exited with {exit_code}: {output!r}")
    rendered = strip_ansi(bytes(output))
    if b"hello" not in rendered:
        raise AssertionError(f"TUI did not render the submitted prompt: {rendered!r}")
    if b"\x1b[?1049l" not in output:
        raise AssertionError("TUI did not restore the alternate screen")
    if not session.is_file():
        raise AssertionError("TUI did not persist its session")
    journal = json_lines(session.read_text(encoding="utf-8"))
    if not any(
        record.get("kind") == "turn"
        and record.get("turn", {}).get("role") == "user"
        and record.get("turn", {}).get("content") == "hello from installed tui"
        for record in journal
    ):
        raise AssertionError(f"TUI did not persist the submitted prompt: {journal!r}")


def main() -> int:
    with tempfile.TemporaryDirectory(prefix="zenpi-user-smoke-") as directory:
        root = Path(directory)
        release = ROOT / "target" / "release" / "zenpi"
        build = run(["cargo", "build", "--release", "--locked"])
        assert_success(build, "release build")
        if not release.is_file() or not os.access(release, os.X_OK):
            raise AssertionError(f"release binary missing: {release}")

        install_root = root / "install"
        install_root.mkdir()
        install = run(["cargo", "install", "--path", ".", "--locked", "--root", str(install_root), "--force"])
        assert_success(install, "isolated cargo install")
        binary = install_root / "bin" / "zenpi"
        if not binary.is_file() or not os.access(binary, os.X_OK):
            raise AssertionError(f"installed binary missing: {binary}")

        help_result = run([str(binary), "--help"])
        assert_success(help_result, "installed help")
        if "tui|headless" not in help_result.stdout:
            raise AssertionError(f"installed help does not expose both modes: {help_result.stdout!r}")

        session = assert_headless_echo(binary, root)
        assert_resume(binary, session, root)
        assert_resume_reopens_process(binary, session, root)
        assert_invalid_inputs(binary, root)
        assert_openai_fixture(binary, root)
        assert_tui(binary, root)
        print("user smoke passed: release, install, echo, resume, OpenAI fixture, and TUI")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
