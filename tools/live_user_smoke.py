#!/usr/bin/env python3
"""Opt-in live-provider smoke against an already paired development profile."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import select
import subprocess
import tempfile
import time
import uuid


class Host:
    def __init__(self, binary, home, profile, workspace, session):
        env = dict(os.environ)
        for key in tuple(env):
            if key.startswith(("ZENPI_", "OPENAI_")):
                del env[key]
        env.update(ZENPI_HOME=str(home), ZENPI_MAX_RETRIES="0", ZENPI_TIMEOUT_SECONDS="120")
        self.stderr = open(workspace / f"stderr-{uuid.uuid4().hex}.log", "xb")
        self.process = subprocess.Popen(
            [str(binary), "--mode", "headless", "--profile", profile, "--session", str(session)],
            cwd=workspace, env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=self.stderr,
        )
        self.pending = b""
        self.events = []

    def send(self, frame):
        self.process.stdin.write((json.dumps(frame) + "\n").encode())
        self.process.stdin.flush()

    def request(self, kind, approve=None, **fields):
        request_id = uuid.uuid4().hex
        self.send(dict(schema_version=2, type=kind, id=request_id, **fields))
        deadline = time.monotonic() + 180
        while time.monotonic() < deadline:
            if b"\n" not in self.pending:
                if not select.select([self.process.stdout], [], [], 0.25)[0]:
                    continue
                chunk = os.read(self.process.stdout.fileno(), 65536)
                if not chunk:
                    raise RuntimeError("live host closed before its terminal response")
                self.pending += chunk
                if len(self.pending) > 2 * 1024 * 1024:
                    raise RuntimeError("live host exceeded the frame bound")
                continue
            line, self.pending = self.pending.split(b"\n", 1)
            if not line:
                continue
            record = json.loads(line)
            event = record.get("event", {})
            if event:
                self.events.append(event.get("type"))
            if event.get("type") == "approval_request":
                approval = event["approval"]
                allowed = approve is not None and approve(approval)
                self.send(dict(schema_version=2, type="approve", id=uuid.uuid4().hex,
                               approval_id=approval["request_id"],
                               decision="allow" if allowed else "deny", remember=False))
            if record.get("type") == "response" and record.get("id") == request_id:
                if not record.get("success"):
                    raise RuntimeError(f"live {kind} failed: {record.get('code')}: {record.get('error')}")
                return record.get("data", {})
        raise TimeoutError(f"live {kind} exceeded 180 seconds")

    def close(self):
        try:
            if self.process.poll() is None:
                self.request("shutdown")
                self.process.wait(timeout=10)
        finally:
            if self.process.poll() is None:
                self.process.terminate()
                try:
                    self.process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    self.process.kill()
                    self.process.wait()
            self.process.stdin.close()
            self.process.stdout.close()
            self.stderr.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--home", type=Path, required=True)
    parser.add_argument("--profile", default="local-codex")
    parser.add_argument("--output-root", type=Path, required=True)
    args = parser.parse_args()
    args.output_root.mkdir(parents=True, exist_ok=True)
    workspace = Path(tempfile.mkdtemp(prefix="live-", dir=args.output_root)).resolve()
    os.chmod(workspace, 0o700)
    nonce = uuid.uuid4().hex[:12]
    secret = uuid.uuid4().hex[:12]
    (workspace / "input.txt").write_text(secret + "\n")
    # `/review` is a git-diff projection; make the isolated fixture a tiny
    # repository so the live command exercises its real path instead of
    # failing merely because tempfile directories are not repositories.
    subprocess.run(["git", "init", "-q"], cwd=workspace, check=True)
    subprocess.run(["git", "config", "user.email", "zenpi-smoke@example.invalid"], cwd=workspace, check=True)
    subprocess.run(["git", "config", "user.name", "zenpi-smoke"], cwd=workspace, check=True)
    subprocess.run(["git", "add", "input.txt"], cwd=workspace, check=True)
    subprocess.run(["git", "commit", "-qm", "fixture"], cwd=workspace, check=True)
    session = workspace / "session.jsonl"
    report = {"workspace": str(workspace), "profile": args.profile,
              "binary_sha256": hashlib.sha256(args.binary.read_bytes()).hexdigest(), "checks": {}}
    host = None
    try:
        host = Host(args.binary, args.home, args.profile, workspace, session)
        answer = host.request("prompt", text=f"Reply exactly ZENPI_LIVE_{nonce}. Do not use tools.")
        text = answer.get("assistant", {}).get("content", "")
        assert f"ZENPI_LIVE_{nonce}" in text, "live provider did not return the requested marker"
        assert "text_delta" in host.events, "live reply did not stream"
        report["checks"]["real_streaming_reply"] = text
        print("PASS real streaming reply:", text, flush=True)

        # Slash controls are local control-plane requests: they must succeed
        # without creating a provider turn and use the same durable host.
        approval = host.request("slash", text="/approval ask")
        assert approval.get("mode") == "ask", approval
        yolo = host.request("slash", text="/yolo on")
        assert yolo.get("enabled") is True, yolo
        goal_help = host.request("slash", text="/help goal")
        assert "separate host primitive" in goal_help.get("text", "")
        # Restore prompting before the approval-crossing fixture below; this
        # keeps the smoke explicit about both posture transitions.
        assert host.request("slash", text="/approval ask").get("mode") == "ask"
        status = host.request("slash", text="/status")
        assert status, "slash status returned no projection"
        review = host.request("slash", text="/review")
        assert review, "slash review returned no projection"
        session = host.request("slash", text="/session list")
        assert session, "slash session returned no projection"
        resume = host.request("slash", text="/resume")
        assert resume, "slash resume returned no projection"
        report["checks"]["slash_status_review_session_resume"] = True
        print("PASS slash status/review/session/resume", flush=True)
        report["checks"]["slash_controls_and_goal_boundary"] = True
        print("PASS slash approval/yolo/help goal", flush=True)

        approved = []

        def allow_workspace_fixture(approval):
            tool, values = approval.get("tool"), approval.get("arguments", {})
            allowed = (tool == "read_file" and values.get("path") == "input.txt") or (
                tool == "write_file" and values.get("path") == "output.txt"
                and values.get("content", "").strip() == secret)
            if allowed:
                approved.append(tool)
            return allowed

        host.request("prompt", approve=allow_workspace_fixture, text=(
            "Use read_file to read input.txt, then use write_file to create output.txt "
            "with exactly the same content. Use relative paths. Do not use run_command "
            "or any other tool. After the tools finish reply FILE_OK."))
        assert (workspace / "output.txt").read_text().strip() == secret
        assert "write_file" in approved, "the file write did not cross live approval"
        report["checks"]["real_read_approved_write_continuation"] = True
        print("PASS real read + approved write + continuation", flush=True)
        host.close()
        host = None

        host = Host(args.binary, args.home, args.profile, workspace, session)
        answer = host.request("prompt", text=(
            "Confirm that this resumed session is usable. Reply exactly SESSION_RESUMED."))
        assert "SESSION_RESUMED" in answer.get("assistant", {}).get("content", ""), answer
        report["checks"]["real_model_recall_after_restart"] = True
        print("PASS real model recall after process restart", flush=True)

        shell = host.request("user_shell", text="!echo ZENPI_SHELL_OK", approve=lambda a:
                             a.get("tool") == "user_shell" and
                             a.get("arguments", {}).get("command") == "echo ZENPI_SHELL_OK")
        assert "ZENPI_SHELL_OK" in json.dumps(shell), f"local shell produced no marker: {shell}"
        report["checks"]["local_bang_shell"] = True
        print("PASS local !echo", flush=True)
        report["success"] = True
    except Exception as error:
        report["success"] = False
        report["error"] = str(error)
        print("FAIL", error, flush=True)
    finally:
        if host:
            host.close()
        (workspace / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        print("Live test receipt:", workspace / "report.json", flush=True)
    return 0 if report.get("success") else 1


if __name__ == "__main__":
    raise SystemExit(main())
