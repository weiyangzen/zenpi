#!/usr/bin/env python3
"""Real TUI/SSE regression: one reply crosses the visual history window."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from tempfile import TemporaryDirectory

from tui_project_workspace_smoke import Terminal, screen_text
from tui_user_shell_smoke import drain


def run_case(binary: Path, evidence: Path, fenced: bool) -> dict:
    gates = [threading.Event(), threading.Event()]
    requests, provider_errors = [], []
    opening = "```text\n" if fenced else ""
    initial = opening + "".join(f"ROW_{i:05}\n" for i in range(8180))
    appended = "".join(f"ROW_{i:05}\n" for i in range(8180, 9180))
    final = "SINGLE_STREAM_FINAL" + ("\n```" if fenced else "")
    expected = initial + appended + final
    assert len(expected.encode()) < 256 * 1024
    result = {"fenced": fenced, "checks": {}, "expected_sha256": hashlib.sha256(expected.encode()).hexdigest()}
    terminal = None

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_POST(self):
            requests.append(json.loads(self.rfile.read(int(self.headers["Content-Length"]))))
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.end_headers()

            def emit(kind, **payload):
                body = json.dumps({"type": kind, **payload})
                self.wfile.write((f"event: {kind}\ndata: {body}\n\n").encode())
                self.wfile.flush()

            try:
                emit("response.created", response={"id": "single-tail", "model": "gpt-4.1"})
                emit("response.output_text.delta", delta=initial)
                if not gates[0].wait(30):
                    raise TimeoutError("reader did not release append gate")
                emit("response.output_text.delta", delta=appended)
                if not gates[1].wait(30):
                    raise TimeoutError("reader did not release finish gate")
                emit("response.output_text.delta", delta=final)
                emit("response.completed", response={"id": "single-tail", "model": "gpt-4.1", "status": "completed"})
            except Exception as error:
                provider_errors.append(repr(error))

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    evidence.mkdir(parents=True, exist_ok=False)
    try:
        with TemporaryDirectory(prefix="single-tail-", dir=Path(".ops").resolve()) as raw:
            root = Path(raw)
            for directory in ("initial", "sessions", "fixture"):
                (root / directory).mkdir()
            config = root / "fixture"
            (config / "config.toml").write_text(
                f'backend="openai"\nmodel="gpt-4.1"\nbase_url="http://127.0.0.1:{server.server_port}/v1"\n'
                'wire_api="responses"\nrequires_openai_auth=false\nmax_retries=0\n'
            )
            (config / "auth.json").write_text(json.dumps({"OPENAI_API_KEY": "single-tail-fixture"}))
            (config / "auth.json").chmod(0o600)
            env = {key: value for key, value in os.environ.items() if not key.startswith(("ZENPI_", "OPENAI_"))}
            env.update(ZENPI_HOME=str(config), TERM="xterm-256color")
            assert env.get("HOME") == os.environ.get("HOME")
            assert env.get("CODEX_HOME") == os.environ.get("CODEX_HOME")
            terminal = Terminal(binary, root, env)
            checkpoint = config / "project-tabs.json"

            def screen():
                return screen_text(bytes(terminal.output)).decode()

            def settled():
                deadline = time.monotonic() + .4
                while time.monotonic() < deadline:
                    drain(terminal.fd, terminal.output, .02)
                return screen()

            def first_row(value):
                return next(line[:55] for line in value.splitlines() if re.search(r"ROW_\d{5}", line[:55]))

            terminal.command("single long reply")
            terminal.expect(b"ROW_08179")
            result["checks"]["initial_tail_visible_before_limit"] = "ROW_08179" in settled()
            terminal.write(b"\x1b[5~")
            terminal.expect(b"Latest")
            before = first_row(settled())
            gates[0].set()
            terminal.wait(lambda: "ROW_09179" in checkpoint.read_text())
            after = first_row(settled())
            result["reading_before"], result["reading_after"] = before, after
            result["checks"]["reading_anchor_survives_limit_crossing"] = before == after
            terminal.write(b"\x1b[F")
            result["after_end_screen"] = settled()
            result["checks"]["end_shows_tail_after_9180_rows"] = "ROW_09179" in result["after_end_screen"]
            gates[1].set()
            journal = root / "sessions/initial.jsonl"
            terminal.wait(lambda: "SINGLE_STREAM_FINAL" in journal.read_text())
            result["final_screen"] = settled()
            result["checks"]["following_shows_final_reply"] = "SINGLE_STREAM_FINAL" in result["final_screen"]
            turns = [json.loads(line) for line in journal.read_text().splitlines()]
            assistants = [row["turn"]["content"] for row in turns if row.get("kind") == "turn" and row["turn"]["role"] == "assistant"]
            result["checks"]["exact_single_assistant_turn_persisted"] = assistants == [expected]
            result["checks"]["exactly_one_streaming_request"] = len(requests) == 1 and requests[0].get("stream") is True
            terminal.close(preserve_draft=True)
            result["checks"]["terminal_restored"] = True
            (evidence / "session.jsonl").write_bytes(journal.read_bytes())
    except Exception as error:
        result["error"] = repr(error)
    finally:
        for gate in gates:
            gate.set()
        if terminal:
            terminal.cleanup()
            (evidence / "terminal.pty.log").write_bytes(terminal.output)
        server.shutdown()
        server.server_close()
        thread.join(timeout=2)
        result["provider_errors"] = provider_errors
        result["passed"] = "error" not in result and not provider_errors and len(result["checks"]) == 7 and all(result["checks"].values())
        (evidence / "http.json").write_text(json.dumps(requests, ensure_ascii=False, indent=2) + "\n")
        (evidence / "result.json").write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n")
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--evidence", type=Path, required=True)
    args = parser.parse_args()
    binary = args.binary.resolve()
    args.evidence.mkdir(parents=True, exist_ok=False)
    results = [run_case(binary, args.evidence / name, fenced) for name, fenced in (("prose", False), ("fenced", True))]
    summary = {"binary": str(binary), "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(), "passed": all(result["passed"] for result in results), "cases": results}
    (args.evidence / "summary.json").write_text(json.dumps(summary, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps({"passed": summary["passed"], "checks": [result["checks"] for result in results]}))
    return 0 if summary["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
