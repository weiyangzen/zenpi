#!/usr/bin/env python3
"""Exercise zenpi as an installed release artifact, not just a Cargo target."""

from __future__ import annotations

import hashlib
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

_ZENPI_ENV_KEYS = {
    "CODEX_HOME",
    "OPENAI_API_KEY",
    "OPENAI_BASE_URL",
    "ZENPI_API_KEY",
    "ZENPI_BACKEND",
    "ZENPI_BASE_URL",
    "ZENPI_HOME",
    "ZENPI_MODEL",
    "ZENPI_PROFILE",
    "ZENPI_SESSION",
    "ZENPI_WIRE_API",
}


def isolated_env(home: Path, **overrides: str) -> dict[str, str]:
    """Retain toolchain variables while excluding host agent credentials/config."""
    env = {key: value for key, value in os.environ.items() if key not in _ZENPI_ENV_KEYS}
    env.update({"HOME": str(home), "ZENPI_HOME": str(home / ".zenpi"), **overrides})
    home.mkdir(parents=True, exist_ok=True)
    return env


def run(
    command: list[str],
    *,
    input_text: str = "",
    env: dict[str, str] | None = None,
    timeout: float = 180,
    cwd: Path = ROOT,
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        command,
        cwd=cwd,
        input=input_text,
        text=True,
        capture_output=True,
        timeout=timeout,
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


def assert_headless_slash_owners(binary: Path, root: Path) -> None:
    """Exercise durable slash owners through the installed binary in a git workspace."""
    workspace = root / "slash-workspace"
    workspace.mkdir()
    for command in (
        ["git", "init", "-q"],
        ["git", "config", "user.email", "zenpi@example.test"],
        ["git", "config", "user.name", "zenpi user smoke"],
    ):
        assert_success(run(command, cwd=workspace), "slash workspace setup")

    tracked = workspace / "tracked.txt"
    attachment = workspace / "note.md"
    tracked.write_text("before\n", encoding="utf-8")
    attachment_text = "installed slash attachment body\n"
    attachment.write_text(attachment_text, encoding="utf-8")
    assert_success(
        run(["git", "add", "tracked.txt", "note.md"], cwd=workspace),
        "slash workspace add",
    )
    assert_success(
        run(["git", "commit", "-qm", "initial"], cwd=workspace),
        "slash workspace commit",
    )
    tracked.write_text("before\nafter\n", encoding="utf-8")

    session = root / "slash-owner-session.jsonl"
    env = isolated_env(root / "slash-user-home")
    first_payload = "\n".join(
        json.dumps(value, separators=(",", ":"))
        for value in (
            {
                "schema_version": 2,
                "type": "command",
                "id": "compete",
                "text": "/compete submit --parent GOAL-1 audit owner boundaries",
            },
            {
                "schema_version": 2,
                "type": "command",
                "id": "models",
                "text": "/models",
            },
            {
                "schema_version": 2,
                "type": "command",
                "id": "doctor",
                "text": "/doctor",
            },
            {
                "schema_version": 2,
                "type": "command",
                "id": "help-doctor",
                "text": "/help doctor",
            },
            {
                "schema_version": 2,
                "type": "command",
                "id": "diff",
                "text": "/diff tracked.txt",
            },
            {
                "schema_version": 2,
                "type": "command",
                "id": "attach",
                "text": "/attach note.md",
            },
            {
                "schema_version": 2,
                "type": "prompt",
                "id": "prompt",
                "text": "use the staged attachment",
            },
            {"schema_version": 2, "type": "shutdown", "id": "shutdown"},
        )
    ) + "\n"
    first = run(
        [
            str(binary),
            "--mode",
            "headless",
            "--backend",
            "echo",
            "--session",
            str(session),
        ],
        input_text=first_payload,
        env=env,
        cwd=workspace,
    )
    assert_success(first, "installed headless diff/attach owners")
    first_records = json_lines(first.stdout)
    first_responses = {
        record["id"]: record
        for record in first_records
        if record.get("type") == "response" and record.get("id")
    }
    for request_id in (
        "compete",
        "models",
        "doctor",
        "help-doctor",
        "diff",
        "attach",
        "prompt",
        "shutdown",
    ):
        response = first_responses.get(request_id, {})
        if response.get("schema_version") != 2 or response.get("success") is not True:
            raise AssertionError(
                f"missing successful v2 slash-owner response for {request_id}: {first_records!r}"
            )

    models = first_responses["models"].get("data", {})
    if models.get("command") != "models" or models.get("route") != "local" or not isinstance(
        models.get("models"), list
    ):
        raise AssertionError(f"installed /models did not return its local catalog: {models!r}")
    doctor = first_responses["doctor"].get("data", {})
    if doctor.get("command") != "doctor" or doctor.get("accepted") is not True:
        raise AssertionError(f"installed /doctor was not a successful local check: {doctor!r}")
    if any(secret in json.dumps(doctor) for secret in ("OPENAI_API_KEY", "sk-")):
        raise AssertionError(f"installed /doctor leaked credential material: {doctor!r}")
    help_doctor = first_responses["help-doctor"].get("data", {})
    if "/doctor" not in help_doctor.get("text", ""):
        raise AssertionError(f"installed /help doctor omitted command metadata: {help_doctor!r}")

    compete = first_responses["compete"].get("data", {})
    if not (
        compete.get("route") == "runtime_intent"
        and compete.get("delivery") == "journal_only"
        and compete.get("zenpi_started") is False
        and compete.get("execution_state") == "untracked"
        and compete.get("created") is True
        and compete.get("idempotent_replay") is False
    ):
        raise AssertionError(f"installed /compete overstated its runtime result: {compete!r}")

    diff = first_responses["diff"].get("data", {})
    if not (
        first_responses["diff"].get("command") == "diff"
        and diff.get("command") == "diff"
        and diff.get("route") == "local"
        and diff.get("accepted") is True
        and diff.get("changed") is True
        and diff.get("path") == "tracked.txt"
        and diff.get("source") == "git"
        and diff.get("truncated") is False
        and "+after" in diff.get("diff", "")
    ):
        raise AssertionError(
            f"installed /diff did not return its structured owner result: {diff!r}"
        )
    if str(workspace) in json.dumps(diff):
        raise AssertionError(f"installed /diff leaked its absolute workspace: {diff!r}")

    expected_digest = hashlib.sha256(attachment_text.encode("utf-8")).hexdigest()
    attached = first_responses["attach"].get("data", {})
    expected_attachment = {
        "command": "attach",
        "accepted": True,
        "path": "note.md",
        "kind": "file",
        "mime_type": "text/plain",
        "size_bytes": len(attachment_text.encode("utf-8")),
        "sha256": expected_digest,
        "pending": 1,
    }
    if any(attached.get(key) != value for key, value in expected_attachment.items()):
        raise AssertionError(
            f"installed /attach did not return its structured owner result: {attached!r}"
        )
    prompt = first_responses["prompt"].get("data", {})
    if prompt.get("assistant", {}).get("content") != "use the staged attachment":
        raise AssertionError(f"prompt after /attach did not complete: {prompt!r}")

    second_payload = "\n".join(
        json.dumps(value, separators=(",", ":"))
        for value in (
            {
                "schema_version": 2,
                "type": "command",
                "id": "compete",
                "text": "/compete submit --parent GOAL-1 audit owner boundaries",
            },
            {
                "schema_version": 2,
                "type": "command",
                "id": "compact",
                "text": "/compact",
            },
            {
                "schema_version": 2,
                "type": "command",
                "id": "resume",
                "text": "/resume 0",
            },
            {"schema_version": 2, "type": "shutdown", "id": "shutdown-2"},
        )
    ) + "\n"
    second = run(
        [
            str(binary),
            "--mode",
            "headless",
            "--backend",
            "echo",
            "--session",
            str(session),
        ],
        input_text=second_payload,
        env=env,
        cwd=workspace,
    )
    assert_success(second, "installed headless compact/resume owners")
    second_records = json_lines(second.stdout)
    second_responses = {
        record["id"]: record
        for record in second_records
        if record.get("type") == "response" and record.get("id")
    }
    for request_id in ("compete", "compact", "resume", "shutdown-2"):
        response = second_responses.get(request_id, {})
        if response.get("schema_version") != 2 or response.get("success") is not True:
            raise AssertionError(
                f"missing successful durable slash response for {request_id}: {second_records!r}"
            )

    compete_replay = second_responses["compete"].get("data", {})
    if not (
        compete_replay.get("created") is False
        and compete_replay.get("idempotent_replay") is True
        and compete_replay.get("intent", {}).get("intent_id")
        == compete.get("intent", {}).get("intent_id")
    ):
        raise AssertionError(
            f"installed runtime intent was not deduplicated across restart: {compete_replay!r}"
        )

    compact = second_responses["compact"].get("data", {})
    if not (
        second_responses["compact"].get("command") == "compact"
        and compact.get("command") == "compact"
        and compact.get("route") == "local"
        and compact.get("accepted") is True
        and compact.get("durable") is True
        and compact.get("already_recorded") is False
        and compact.get("source_turns") == 2
        and compact.get("prepared_turns") == 2
        and isinstance(compact.get("marker_sequence"), int)
        and compact.get("next_sequence") == compact["marker_sequence"] + 1
    ):
        raise AssertionError(f"installed /compact did not persist a structured result: {compact!r}")

    resumed = second_responses["resume"].get("data", {})
    replay = resumed.get("records", [])
    if not (
        second_responses["resume"].get("command") == "resume"
        and resumed.get("command") == "resume"
        and resumed.get("route") == "local"
        and resumed.get("accepted") is True
        and resumed.get("durable") is True
        and resumed.get("requested_sequence") == 0
        and resumed.get("from_sequence") == 0
        and resumed.get("replay_gap") is False
        and resumed.get("truncated") is False
        and resumed.get("replayed") == len(replay)
        and resumed.get("marker_sequence") == compact.get("next_sequence")
        and resumed.get("next_sequence") == resumed["marker_sequence"] + 1
    ):
        raise AssertionError(f"installed /resume did not return a durable replay: {resumed!r}")
    if not any(
        record.get("kind") == "turn"
        and record.get("role") == "user"
        and record.get("content") == "use the staged attachment"
        for record in replay
    ):
        raise AssertionError(f"/resume did not replay the prior process turn: {replay!r}")
    if not any(
        record.get("kind") == "event"
        and record.get("event", {}).get("trigger") == "manual_slash"
        and record.get("event", {}).get("type")
        in {"context_compacted", "context_compaction_skipped"}
        for record in replay
    ):
        raise AssertionError(f"/resume did not replay the durable compact marker: {replay!r}")

    journal_text = session.read_text(encoding="utf-8")
    journal = json_lines(journal_text)
    user_turns = [
        record.get("turn", {})
        for record in journal
        if record.get("kind") == "turn" and record.get("turn", {}).get("role") == "user"
    ]
    if len(user_turns) != 1:
        raise AssertionError(f"slash-owner session has unexpected user turns: {journal!r}")
    attachment_metadata = user_turns[0].get("metadata", {}).get("attachments", [])
    if len(attachment_metadata) != 1 or any(
        attachment_metadata[0].get(key) != value
        for key, value in {
            "path": "note.md",
            "kind": "file",
            "mime_type": "text/plain",
            "size_bytes": len(attachment_text.encode("utf-8")),
            "sha256": expected_digest,
        }.items()
    ):
        raise AssertionError(
            f"staged attachment was not consumed by exactly the next durable turn: {user_turns!r}"
        )
    if attachment_text.strip() in journal_text:
        raise AssertionError("attachment bytes were persisted instead of bounded metadata")
    durable_events = [
        record.get("event", {}) for record in journal if record.get("kind") == "event"
    ]
    if not any(
        event.get("type") in {"context_compacted", "context_compaction_skipped"}
        and event.get("trigger") == "manual_slash"
        for event in durable_events
    ):
        raise AssertionError(f"/compact marker is absent from the durable journal: {journal!r}")
    if not any(
        event.get("type") == "session_resumed"
        and event.get("trigger") == "manual_slash"
        and event.get("requested_sequence") == 0
        for event in durable_events
    ):
        raise AssertionError(f"/resume marker is absent from the durable journal: {journal!r}")
    runtime_intents = [
        record for record in journal if record.get("kind") == "runtime_intent"
    ]
    if len(runtime_intents) != 1:
        raise AssertionError(f"cross-process runtime intent was appended twice: {runtime_intents!r}")


def assert_invalid_inputs(binary: Path, root: Path) -> None:
    missing_key_session = root / "missing-key.jsonl"
    isolated_home = root / "empty-home"
    isolated_home.mkdir()
    env = isolated_env(isolated_home)
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
            "input": body.get("input"),
        }
        if self.path != "/v1/responses" or body.get("stream") is not True:
            self.send_error(400, "Responses fixture requires /v1/responses with stream=true")
            return
        response = (
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp-user-smoke\",\"model\":\"mock-model\"}}\n\n"
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"provider works\"}\n\n"
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-user-smoke\",\"model\":\"mock-model\",\"usage\":{\"input_tokens\":3,\"output_tokens\":2,\"total_tokens\":5}}}\n\n"
            "data: [DONE]\n"
        ).encode("utf-8")
        self.send_response(200)
        self.send_header("content-type", "text/event-stream")
        self.send_header("content-length", str(len(response)))
        self.send_header("connection", "close")
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
    env = isolated_env(
        root / "provider-home",
        ZENPI_BASE_URL=f"http://127.0.0.1:{server.server_port}/v1",
        ZENPI_API_KEY="user-smoke-key",
        ZENPI_WIRE_API="responses",
    )
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
    if request.get("path") != "/v1/responses":
        raise AssertionError(f"unexpected provider path: {request!r}")
    if request.get("model") != "mock-model" or request.get("stream") is not True:
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
        status = terminate_pty_child(pid)
        if status is None:
            raise AssertionError(
                "TUI did not exit before deadline and could not be reaped; "
                f"output={bytes(output)!r}"
            )
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


def terminate_pty_child(pid: int, grace: float = 0.5) -> int | None:
    """Terminate and reap a PTY child without blocking the smoke runner.

    A child can be blocked in a full-screen write when the test harness has
    not drained the master side. Waiting unconditionally after SIGTERM would
    then strand the whole smoke run, so use a short graceful window followed
    by SIGKILL and non-blocking reap attempts.
    """
    try:
        os.kill(pid, signal.SIGTERM)
    except (OSError, ProcessLookupError):
        pass
    deadline = time.monotonic() + max(grace, 0.0)
    while time.monotonic() < deadline:
        try:
            waited, status = os.waitpid(pid, os.WNOHANG)
        except (OSError, ChildProcessError):
            return None
        if waited == pid:
            return status
        time.sleep(0.02)
    try:
        os.kill(pid, signal.SIGKILL)
    except (OSError, ProcessLookupError):
        pass
    deadline = time.monotonic() + 1.0
    while time.monotonic() < deadline:
        try:
            waited, status = os.waitpid(pid, os.WNOHANG)
        except (OSError, ChildProcessError):
            return None
        if waited == pid:
            return status
        time.sleep(0.02)
    return None


def drain_pty(fd: int, output: bytearray, timeout: float = 0.0) -> bool:
    """Drain queued PTY output without blocking.

    Production terminals continuously consume redraws. The smoke harness must
    do the same while it waits on a journal file; otherwise a multiline
    prompt can fill the PTY buffer and stop the child before it reads Enter.
    """
    drained = False
    deadline = time.monotonic() + max(timeout, 0.0)
    while True:
        if timeout > 0 and time.monotonic() >= deadline:
            break
        remaining = deadline - time.monotonic()
        wait = min(0.05, remaining) if timeout > 0 else 0.0
        if wait < 0:
            wait = 0.0
        ready, _, _ = select.select([fd], [], [], wait)
        if not ready:
            break
        try:
            chunk = os.read(fd, 65536)
        except OSError:
            break
        if not chunk:
            break
        output.extend(chunk)
        drained = True
    return drained


def wait_for_tui_turn(
    session: Path,
    timeout: float = 10,
    *,
    fd: int | None = None,
    output: bytearray | None = None,
) -> None:
    """Wait for the synchronous echo turn to be durable before quitting.

    When a PTY is supplied, drain redraws during the journal wait just as a
    real terminal would. Keeping this optional preserves the helper's use in
    non-PTY checks while preventing output back-pressure in interactive tests.
    """
    if (fd is None) != (output is None):
        raise ValueError("fd and output must be provided together")
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if fd is not None and output is not None:
            drain_pty(fd, output, min(0.05, max(0.0, deadline - time.monotonic())))
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
        time.sleep(0.01)
    raise AssertionError("TUI did not persist the submitted turn before exit")


def strip_ansi(data: bytes) -> bytes:
    """Remove terminal control sequences before inspecting PTY text."""
    return re.sub(rb"\x1b\[[0-?]*[ -/]*[@-~]", b"", data).replace(b"\r", b"")


def assert_tui(binary: Path, root: Path) -> None:
    session = root / "tui-session.jsonl"
    command = [str(binary), "--mode", "tui", "--backend", "echo", "--session", str(session)]
    pid, fd = pty.fork()
    if pid == 0:
        os.execve(command[0], command, isolated_env(root / "tui-home"))
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
        # Exercise a real resize before typing; the render loop must remain
        # live and restore the terminal after the changed geometry.
        fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 18, 72, 0, 0))
        os.kill(pid, signal.SIGWINCH)
        os.write(fd, b"hello from installed tui")
        os.write(fd, b"\r")
        # Enter synchronously completes the deterministic echo turn. Wait for
        # its journal record so a slow CI host cannot race Ctrl-C with Enter.
        wait_for_tui_turn(session, fd=fd, output=output)
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
            terminate_pty_child(pid)
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


def assert_tui_multiline_paste(binary: Path, root: Path) -> None:
    """Exercise bracketed paste and multiline submission through a real PTY.

    Unit tests cover the key-level editing paths, but a terminal can deliver a
    paste as one crossterm ``Event::Paste``.  This check makes sure the
    installed binary keeps the embedded newlines, submits the complete text,
    and still restores the alternate screen after the turn.
    """
    session = root / "tui-multiline-session.jsonl"
    command = [str(binary), "--mode", "tui", "--backend", "echo", "--session", str(session)]
    pid, fd = pty.fork()
    if pid == 0:
        os.execve(command[0], command, isolated_env(root / "tui-multiline-home"))
    exited = False
    output = bytearray()
    expected = "first pasted line\nsecond pasted line\nthird pasted line"
    try:
        # Keep enough rows for all pasted lines while still exercising the
        # responsive prompt layout rather than relying on the default PTY.
        fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 20, 80, 0, 0))
        ready_deadline = time.monotonic() + 5
        while time.monotonic() < ready_deadline and b"Prompt" not in output:
            ready, _, _ = select.select([fd], [], [], 0.1)
            if ready:
                try:
                    output.extend(os.read(fd, 65536))
                except OSError:
                    break
        if b"Prompt" not in output:
            raise AssertionError(f"multiline TUI did not reach prompt: {bytes(output)!r}")

        # Crossterm's bracketed-paste protocol is enabled by TerminalGuard.
        # Send the markers separately from Enter so the test also exercises
        # the paste event boundary instead of depending on packet grouping.
        os.write(
            fd,
            b"\x1b[200~first pasted line\nsecond pasted line\nthird pasted line\x1b[201~",
        )
        time.sleep(0.1)
        os.write(fd, b"\r")
        wait_for_tui_turn(session, fd=fd, output=output)

        # Ctrl-D is the deterministic empty-prompt quit binding across PTY
        # implementations; Ctrl-C is intentionally reserved for interrupt.
        os.write(fd, b"\x04")
        exit_code, trailing = read_pty_until_exit(pid, fd, time.monotonic() + 10)
        exited = True
        output.extend(trailing)
    except BaseException:
        if not exited:
            terminate_pty_child(pid)
        raise
    finally:
        try:
            os.close(fd)
        except OSError:
            pass
    if exit_code != 0:
        raise AssertionError(f"multiline TUI exited with {exit_code}: {output!r}")
    if b"\x1b[?1049l" not in output:
        raise AssertionError("multiline TUI did not restore the alternate screen")
    if not session.is_file():
        raise AssertionError("multiline TUI did not persist its session")
    journal = json_lines(session.read_text(encoding="utf-8"))
    user_turns = [
        record.get("turn", {})
        for record in journal
        if record.get("kind") == "turn" and record.get("turn", {}).get("role") == "user"
    ]
    if not any(turn.get("content") == expected for turn in user_turns):
        raise AssertionError(
            f"bracketed paste did not preserve multiline prompt: {user_turns!r}"
        )


def assert_tui_interrupt_while_streaming(binary: Path, root: Path) -> None:
    """Drive the production TUI against a deliberately slow Responses stream."""
    class SlowHandler(BaseHTTPRequestHandler):
        def do_POST(self) -> None:  # noqa: N802
            length = int(self.headers.get("content-length", "0"))
            self.rfile.read(length)
            first = (
                'data: {"type":"response.output_text.delta","delta":"streaming"}\n\n'
            ).encode()
            tail = (
                'data: {"type":"response.completed","response":{"id":"slow"}}\n\n'
            ).encode()
            self.send_response(200)
            self.send_header("content-type", "text/event-stream")
            self.send_header("connection", "close")
            self.end_headers()
            self.wfile.write(first)
            self.wfile.flush()
            time.sleep(0.5)
            try:
                self.wfile.write(tail)
                self.wfile.flush()
            except BrokenPipeError:
                pass

        def log_message(self, *_args: Any) -> None:
            return

    server = ThreadingHTTPServer(("127.0.0.1", 0), SlowHandler)
    server_thread = threading.Thread(target=server.handle_request, daemon=True)
    server_thread.start()
    session = root / "tui-stream-session.jsonl"
    command = [str(binary), "--mode", "tui", "--session", str(session)]
    env = isolated_env(
        root / "tui-stream-home",
        ZENPI_BASE_URL=f"http://127.0.0.1:{server.server_port}/v1",
        ZENPI_API_KEY="user-smoke-key",
        ZENPI_WIRE_API="responses",
        ZENPI_MODEL="mock-model",
    )
    pid, fd = pty.fork()
    if pid == 0:
        os.execve(command[0], command, env)
    exited = False
    output = bytearray()
    try:
        fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 100, 0, 0))
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline and b"Prompt" not in output:
            ready, _, _ = select.select([fd], [], [], 0.1)
            if ready:
                output.extend(os.read(fd, 65536))
        os.write(fd, b"slow request\r")
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline and b"streaming" not in strip_ansi(bytes(output)):
            ready, _, _ = select.select([fd], [], [], 0.1)
            if ready:
                output.extend(os.read(fd, 65536))
        if b"streaming" not in strip_ansi(bytes(output)):
            raise AssertionError("TUI did not render a provider delta")
        os.write(fd, b"\x03")
        time.sleep(0.15)
        os.write(fd, b"\x04")
        exit_code, trailing = read_pty_until_exit(pid, fd, time.monotonic() + 10)
        exited = True
        output.extend(trailing)
    finally:
        if not exited:
            terminate_pty_child(pid)
        os.close(fd)
        server_thread.join(timeout=3)
        server.server_close()
    if exit_code != 0 or b"\x1b[?1049l" not in output:
        raise AssertionError(f"streaming TUI did not restore terminal: {output!r}")


def main() -> int:
    with tempfile.TemporaryDirectory(prefix="zenpi-user-smoke-") as directory:
        root = Path(directory)
        release = ROOT / "target" / "release" / "zenpi"
        features = os.environ.get("ZENPI_SMOKE_FEATURES", "dev-fixtures")
        feature_args = ["--features", features] if features else []
        build = run(["cargo", "build", "--release", "--locked", *feature_args])
        assert_success(build, "release build")
        if not release.is_file() or not os.access(release, os.X_OK):
            raise AssertionError(f"release binary missing: {release}")

        install_root = root / "install"
        install_root.mkdir()
        install = run(
            [
                "cargo",
                "install",
                "--path",
                ".",
                "--locked",
                "--root",
                str(install_root),
                "--force",
                *feature_args,
            ]
        )
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
        assert_headless_slash_owners(binary, root)
        assert_invalid_inputs(binary, root)
        assert_openai_fixture(binary, root)
        assert_tui(binary, root)
        assert_tui_multiline_paste(binary, root)
        assert_tui_interrupt_while_streaming(binary, root)
    print(
        "user smoke passed: release, install, echo fixture, durable slash/runtime intents, "
        "resume, Responses fixture, TUI resize/multiline paste, streaming interrupt, "
        "and terminal restoration"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
