#!/usr/bin/env bash
# Exercise the public headless boundary without requiring model credentials.
# The test talks to the compiled binary through stdin/stdout so accidental
# diagnostics on stdout and session writes after a response are observable.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
ALLOW_SKIP=0
BIN="${ZENPI_BIN:-}"
RELEASE_CHECK=0

usage() {
  cat <<'EOF'
Usage: tools/headless_smoke.sh [--allow-skip] [--release] [--bin PATH]

Run a deterministic echo-backend JSONL smoke test. The test fails when the
Rust toolchain or binary is unavailable unless --allow-skip (or
ZENPI_SMOKE_ALLOW_SKIP=1) is supplied.
EOF
}

while (($# > 0)); do
  case "$1" in
    --allow-skip)
      ALLOW_SKIP=1
      shift
      ;;
    --bin)
      (($# >= 2)) || { echo "headless smoke: --bin needs a path" >&2; exit 2; }
      BIN="$2"
      shift 2
      ;;
    --release)
      RELEASE_CHECK=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "headless smoke: unknown option: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [[ "${ZENPI_SMOKE_ALLOW_SKIP:-0}" == "1" ]]; then
  ALLOW_SKIP=1
fi

skip() {
  echo "headless smoke: skipped ($*)" >&2
  exit 0
}

fail() {
  echo "headless smoke: ERROR: $*" >&2
  exit 1
}

if [[ -z "$BIN" ]]; then
  if ! command -v cargo >/dev/null 2>&1; then
    ((ALLOW_SKIP)) && skip "cargo is not installed"
    fail "cargo is not installed"
  fi
  BUILD_ARGS=(build --quiet --manifest-path "$ROOT_DIR/Cargo.toml" --bin zenpi)
  if ((RELEASE_CHECK)); then
    BUILD_ARGS=(build --quiet --locked --release --manifest-path "$ROOT_DIR/Cargo.toml" --bin zenpi)
  fi
  cargo "${BUILD_ARGS[@]}" || {
    ((ALLOW_SKIP)) && skip "cargo build failed"
    fail "cargo build failed"
  }
  if ((RELEASE_CHECK)); then
    BIN="$ROOT_DIR/target/release/zenpi"
  else
    BIN="$ROOT_DIR/target/debug/zenpi"
  fi
fi

[[ -x "$BIN" ]] || {
  ((ALLOW_SKIP)) && skip "binary is not executable: $BIN"
  fail "binary is not executable: $BIN"
}

TMP_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/zenpi-headless-smoke.XXXXXX")"
trap 'rm -rf "$TMP_ROOT"' EXIT
SESSION="$TMP_ROOT/session.jsonl"
INPUT="$TMP_ROOT/input.jsonl"
OUTPUT="$TMP_ROOT/output.jsonl"
STDERR_FILE="$TMP_ROOT/stderr.log"

# Start with a rejected command. The process must remain usable afterwards;
# Unicode line-separator escapes must remain payload characters, not framing.
printf '%s\n' \
  '{not-json}' \
  '{"type":"unknown_smoke_command","id":"bad-1"}' \
  '{"type":"prompt","id":"p-1","text":"hello\u2028zenpi\u2029","mode":"start_if_idle","future_hint":"ignored"}' \
  '{"type":"status","id":"s-1"}' \
  '{"type":"steer","id":"t-1","text":"no active turn"}' \
  '{"type":"resume","id":"r-1"}' \
  '{"type":"handoff","id":"h-1","to":"worker-b","summary":"handoff smoke","artifacts":["Docs/Zenpi_Execution_Spec.md"]}' \
  '{"type":"shutdown","id":"q-1"}' >"$INPUT"

# ZENPI_SESSION is the stable persistence boundary. The CLI also advertises
# --session; use that flag when present so the check follows the public help.
HELP="$({ "$BIN" --help 2>&1 || true; } | LC_ALL=C tr '\n' ' ')"
CLI_ARGS=(--mode headless)
if [[ "$HELP" == *"--backend"* ]]; then
  CLI_ARGS+=(--backend echo)
fi
if [[ "$HELP" == *"--session-path"* ]]; then
  CLI_ARGS+=(--session-path "$SESSION")
elif [[ "$HELP" == *"--session"* ]]; then
  CLI_ARGS+=(--session "$SESSION")
fi

set +e
ZENPI_SESSION="$SESSION" "$BIN" "${CLI_ARGS[@]}" <"$INPUT" >"$OUTPUT" 2>"$STDERR_FILE"
STATUS=$?
set -e
((STATUS == 0)) || {
  sed 's/^/  /' "$STDERR_FILE" >&2 || true
  fail "headless process exited with status $STATUS"
}

python3 - "$OUTPUT" "$SESSION" "$BIN" "$TMP_ROOT" <<'PY'
"""Validate the smoke transcript and durable session with only stdlib."""
from __future__ import annotations

import json
import os
import pathlib
import subprocess
import sys

output_path, session_path, binary, tmp_root = map(pathlib.Path, sys.argv[1:])


def read_jsonl(path: pathlib.Path) -> list[dict]:
    if not path.exists():
        raise AssertionError(f"missing JSONL file: {path}")
    records: list[dict] = []
    # JSONL framing is *only* LF. str.splitlines() would incorrectly treat
    # U+2028/U+2029 payload characters as record boundaries.
    lines = path.read_text(encoding="utf-8").split("\n")
    if lines and lines[-1] == "":
        lines.pop()
    for line_no, raw in enumerate(lines, 1):
        if raw.endswith("\r"):
            raw = raw[:-1]
        if line_no == 1 and raw.startswith("\ufeff"):
            raw = raw[1:]
        if not raw.strip():
            raise AssertionError(f"blank protocol line {line_no}")
        try:
            value = json.loads(raw)
        except json.JSONDecodeError as exc:
            raise AssertionError(f"line {line_no} is not JSON: {exc}") from exc
        if not isinstance(value, dict):
            raise AssertionError(f"line {line_no} is not a JSON object")
        records.append(value)
    return records


responses = read_jsonl(output_path)
if not responses:
    raise AssertionError("headless process emitted no responses")
for index, response in enumerate(responses, 1):
    # Responses and typed lifecycle events are both valid stdout records. A
    # diagnostic string or an untyped object would violate the JSONL contract.
    if not isinstance(response.get("type"), str) or not response["type"]:
        raise AssertionError(f"line {index} has no typed protocol record")

by_id: dict[str, dict] = {}
for response in responses:
    if response.get("type") != "response":
        continue
    request_id = response.get("id")
    if request_id is not None:
        if request_id in by_id:
            raise AssertionError(f"duplicate correlated response for {request_id}")
        by_id[request_id] = response
for request_id in ("bad-1", "p-1", "s-1", "t-1", "r-1", "h-1", "q-1"):
    if request_id not in by_id:
        raise AssertionError(f"missing correlated response for {request_id}")
expected_commands = {
    "bad-1": "unknown_smoke_command",
    "p-1": "prompt",
    "s-1": "status",
    "t-1": "steer",
    "r-1": "resume",
    "h-1": "handoff",
    "q-1": "shutdown",
}
for request_id, command in expected_commands.items():
    if by_id[request_id].get("command") != command:
        raise AssertionError(
            f"response {request_id} has command {by_id[request_id].get('command')!r}, "
            f"expected {command!r}"
        )
if by_id["bad-1"].get("success") is not False:
    raise AssertionError("unknown command was not rejected")
invalid_frames = [response for response in responses if response.get("command") == "invalid"]
if not invalid_frames or any(response.get("success") is not False for response in invalid_frames):
    raise AssertionError("malformed JSON frame was not rejected without terminating the loop")
for request_id in ("p-1", "s-1", "t-1", "r-1", "h-1", "q-1"):
    if by_id[request_id].get("success") is not True:
        raise AssertionError(f"command {request_id} did not succeed: {by_id[request_id]}")

session_records = read_jsonl(session_path)
if len(session_records) < 3:
    raise AssertionError("session journal is unexpectedly short")
kinds = [record.get("kind") for record in session_records]
session_ids = {record.get("session_id") for record in session_records if record.get("session_id")}
if len(session_ids) != 1:
    raise AssertionError("session records do not share one session_id")
sequences = [record.get("seq") for record in session_records]
if any(not isinstance(sequence, int) for sequence in sequences) or any(
    left >= right for left, right in zip(sequences, sequences[1:])
):
    raise AssertionError(f"session sequence is not strictly monotonic: {sequences!r}")
if any(not isinstance(record.get("timestamp_ms"), int) for record in session_records):
    raise AssertionError("session record is missing an integer timestamp_ms")
if "session" not in kinds:
    raise AssertionError("session journal has no session record")
if "handoff" not in kinds and "handoff_record" not in kinds:
    raise AssertionError("session journal has no handoff record")
if kinds.count("turn") != 2:
    raise AssertionError(f"invalid command changed turn count: {kinds.count('turn')}")

# Invalid modes must fail before opening a session or backend.
for mode in ("print", "json", "rpc", "server", "daemon"):
    invalid_session = pathlib.Path(tmp_root) / f"invalid-{mode}.jsonl"
    probe = subprocess.run(
        [str(binary), "--mode", mode, "--backend", "echo"],
        input="",
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env={**os.environ, "ZENPI_SESSION": str(invalid_session)},
        check=False,
    )
    if probe.returncode == 0:
        raise AssertionError(f"forbidden {mode} mode was accepted")
    if invalid_session.exists():
        raise AssertionError(f"invalid {mode} mode opened a session before rejection")

print(f"headless smoke passed: {len(responses)} responses, {len(session_records)} session records")
PY

# Exercise the bounded reader separately: an overlong frame and invalid UTF-8
# must be drained/rejected while a following valid request still completes.
python3 - "$BIN" "$TMP_ROOT" <<'PY'
import json
import pathlib
import subprocess
import sys

binary, root = sys.argv[1], pathlib.Path(sys.argv[2])
session = root / "bounded-session.jsonl"
payload = b"x" * (1024 * 1024 + 1) + b"\n\xff\n"
payload += b'{"type":"shutdown","id":"bounded-ok"}\n'
run = subprocess.run(
    [binary, "--mode", "headless", "--backend", "echo", "--session", str(session)],
    input=payload,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    check=False,
)
if run.returncode != 0:
    raise AssertionError(run.stderr.decode("utf-8", "replace"))
records = [json.loads(line) for line in run.stdout.splitlines()]
if records[0].get("code") != "line_too_long":
    raise AssertionError(f"overlong frame was not rejected: {records!r}")
if records[1].get("code") != "invalid_utf8":
    raise AssertionError(f"invalid UTF-8 was not rejected: {records!r}")
if not any(record.get("id") == "bounded-ok" and record.get("success") for record in records):
    raise AssertionError("valid request after bounded failures did not complete")
PY

echo "headless smoke: passed"
