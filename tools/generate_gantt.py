#!/usr/bin/env python3
"""Regenerate the read-only same-prefix Gantt projection from the Blueprint."""

from __future__ import annotations

import datetime as dt
import hashlib
import os
import re
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BLUEPRINT = ROOT / "Docs/Zenpi_Execution_Blueprint.md"
GANTT = ROOT / "Docs/Zenpi_Execution_Gantt.md"
STATES = {" ": "unclaimed", "_": "self_tested", "x": "master_accepted"}
ROW = re.compile(r"^- \[([ _x])\] \*\*(ZP-[0-9]{3})\*\*")


def main() -> int:
    blueprint = BLUEPRINT.read_text(encoding="utf-8")
    state_by_id = {match.group(2): STATES[match.group(1)] for match in map(ROW.match, blueprint.splitlines()) if match}
    if not state_by_id:
        raise SystemExit("gantt generator: Blueprint has no stable rows")
    text = GANTT.read_text(encoding="utf-8")
    counts = {state: sum(value == state for value in state_by_id.values()) for state in STATES.values()}
    replacements = {
        "generated_at": dt.datetime.now(dt.UTC).replace(microsecond=0).isoformat().replace("+00:00", "Z"),
        "source_sha256": hashlib.sha256(BLUEPRINT.read_bytes()).hexdigest(),
        "spec_sha256": hashlib.sha256((ROOT / "Docs/Zenpi_Execution_Spec.md").read_bytes()).hexdigest(),
        "unclaimed": str(counts["unclaimed"]),
        "self_tested": str(counts["self_tested"]),
        "master_accepted": str(counts["master_accepted"]),
    }
    for key, value in replacements.items():
        text, changed = re.subn(rf"^(\s*{re.escape(key)}:) .*?$", rf"\1 {value}", text, count=1, flags=re.MULTILINE)
        if changed != 1:
            raise SystemExit(f"gantt generator: missing header field {key}")
    for item_id, state in state_by_id.items():
        text, changed = re.subn(rf"^(\| {re.escape(item_id)} \| )[^|]+( \|)", rf"\1{state}\2", text, count=1, flags=re.MULTILINE)
        if changed != 1:
            raise SystemExit(f"gantt generator: missing monitoring row {item_id}")
    fd, temporary = tempfile.mkstemp(prefix=".zenpi-gantt.", dir=GANTT.parent)
    try:
        with os.fdopen(fd, "w", encoding="utf-8", newline="") as handle:
            handle.write(text)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary, GANTT)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)
    print(f"generated {GANTT.relative_to(ROOT)} ({len(state_by_id)} rows)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
