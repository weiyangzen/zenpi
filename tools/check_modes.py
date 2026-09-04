#!/usr/bin/env python3
"""Check the executable's deliberately tiny public mode boundary."""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FORBIDDEN = ("print", "json", "rpc", "server", "daemon")


def main() -> int:
    core = (ROOT / "src/core.rs").read_text(encoding="utf-8")
    match = re.search(r"pub enum RunMode\s*\{(.*?)\n\}", core, re.DOTALL)
    if not match or re.findall(r"^\s*(Tui|Headless),?\s*$", match.group(1), re.MULTILINE) != [
        "Tui",
        "Headless",
    ]:
        print("mode check: RunMode is not exactly Tui/Headless", file=sys.stderr)
        return 1
    if re.search(r"RunMode::(?:Print|Json|Rpc|Server|Daemon)", core):
        print("mode check: forbidden RunMode variant", file=sys.stderr)
        return 1
    binary = ROOT / "target/debug/zenpi"
    subprocess.run(
        ["cargo", "build", "--quiet", "--manifest-path", str(ROOT / "Cargo.toml")],
        check=True,
    )
    help_text = subprocess.run([str(binary), "--help"], text=True, capture_output=True, check=False).stdout.lower()
    if "tui" not in help_text or "headless" not in help_text:
        print("mode check: help does not advertise both public modes", file=sys.stderr)
        return 1
    # `--json` is a config-output flag, not a runtime mode. Reject only mode
    # spellings/commands rather than arbitrary substrings in option names.
    advertised_modes = set(re.findall(r"--mode\s+([a-z|]+)", help_text))
    if any(token in advertised_modes for token in FORBIDDEN) or any(
        re.search(rf"\bzenpi\s+{re.escape(token)}\b", help_text) for token in FORBIDDEN
    ):
        print("mode check: help advertises a forbidden mode", file=sys.stderr)
        return 1
    print("mode check: exactly tui/headless")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
