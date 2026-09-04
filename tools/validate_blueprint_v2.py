#!/usr/bin/env python3
"""Validate the non-authoritative v2 Blueprint review draft.

The frozen v1 validator intentionally checks only the authoritative v1
document.  v2 is kept as a side-by-side review artifact, so it gets a small
independent checker for the claims that matter during review: version
metadata, unique V2 IDs, valid states, dependency closure/acyclicity, and the
strict per-item LOC forecast cap.  This uses only the Python standard library.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any


DEFAULT_PATH = Path("Docs/Zenpi_Execution_Blueprint_v2.md")
DEFAULT_CAP = 5_000
STATES = {"AUDITED", "PARTIAL", "PLANNED", "DEFERRED", "ACCEPTED"}


class BlueprintV2Error(Exception):
    """A deterministic review-draft validation failure."""


def _scalar(value: str) -> Any:
    value = value.strip()
    if value.lower() in {"true", "false"}:
        return value.lower() == "true"
    if re.fullmatch(r"[0-9]+", value):
        return int(value)
    if (value.startswith("'") and value.endswith("'")) or (
        value.startswith('"') and value.endswith('"')
    ):
        return value[1:-1]
    return value


def _header(text: str) -> dict[str, Any]:
    matches = re.findall(r"```yaml\s*\n(.*?)\n```", text, re.DOTALL)
    if not matches:
        raise BlueprintV2Error("missing fenced yaml header")
    values: dict[str, Any] = {}
    for raw in matches[0].splitlines():
        raw = raw.strip()
        if not raw or raw.startswith("#"):
            continue
        key, separator, value = raw.partition(":")
        if not separator or not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", key):
            raise BlueprintV2Error(f"invalid yaml header line: {raw!r}")
        if key in values:
            raise BlueprintV2Error(f"duplicate yaml key: {key}")
        values[key] = _scalar(value)
    return values


def _split_row(line: str) -> list[str]:
    # The v2 table uses backticks for paths but does not use literal pipes in
    # cells.  Keep this parser deliberately strict so a malformed row cannot
    # accidentally pass with a shifted Estimated LOC value.
    cells = [cell.strip() for cell in line.strip().strip("|").split("|")]
    if len(cells) != 7:
        raise BlueprintV2Error(f"expected 7 table cells, got {len(cells)}: {line!r}")
    return cells


def _rows(text: str) -> dict[str, tuple[str, tuple[str, ...], int, int]]:
    rows: dict[str, tuple[str, tuple[str, ...], int, int]] = {}
    for line_number, line in enumerate(text.splitlines(), 1):
        # Do not silently skip a table row that looks like a v2 item but has
        # a malformed stable ID.  A typo in an ID must fail closed rather
        # than making the row disappear from the validator's accounting.
        if re.match(r"^\s*\|\s*V2-", line) and not re.match(
            r"^\s*\|\s*V2-[0-9]{3}\s*\|", line
        ):
            raise BlueprintV2Error(f"line {line_number}: malformed V2 checklist ID")
        if not re.match(r"^\s*\|\s*V2-[0-9]{3}\s*\|", line):
            continue
        cells = _split_row(line)
        state = cells[1]
        if state not in STATES:
            raise BlueprintV2Error(f"line {line_number}: {cells[0]} has invalid state {state!r}")
        try:
            loc = int(cells[6])
        except ValueError as exc:  # pragma: no cover - regex catches this
            raise BlueprintV2Error(f"line {line_number}: {cells[0]} has invalid LOC") from exc
        if cells[0] in rows:
            raise BlueprintV2Error(f"duplicate ID {cells[0]}")
        dependency_text = cells[4].strip().strip("`")
        dependencies = tuple(
            item.strip().strip("`")
            for item in dependency_text.split(",")
            if item.strip() and item.strip() not in {"-", "none", "None"}
        )
        rows[cells[0]] = (state, dependencies, loc, line_number)
    if not rows:
        raise BlueprintV2Error("no V2 checklist rows found")
    return rows


def _check_cycles(rows: dict[str, tuple[str, tuple[str, ...], int, int]]) -> None:
    for item_id, (_, dependencies, _, line_number) in rows.items():
        for dependency in dependencies:
            if dependency not in rows:
                raise BlueprintV2Error(
                    f"line {line_number}: {item_id} depends on missing {dependency}"
                )
    visiting: set[str] = set()
    visited: set[str] = set()

    def visit(item_id: str) -> None:
        if item_id in visiting:
            raise BlueprintV2Error(f"dependency cycle detected at {item_id}")
        if item_id in visited:
            return
        visiting.add(item_id)
        for dependency in rows[item_id][1]:
            visit(dependency)
        visiting.remove(item_id)
        visited.add(item_id)

    for item_id in rows:
        visit(item_id)


def validate(path: Path = DEFAULT_PATH) -> dict[str, Any]:
    try:
        text = path.read_text(encoding="utf-8")
    except OSError as exc:
        raise BlueprintV2Error(f"cannot read {path}: {exc}") from exc
    header = _header(text)
    if header.get("schema_version") != "execution-blueprint/v2":
        raise BlueprintV2Error("schema_version must be execution-blueprint/v2")
    if header.get("authoritative") is not False:
        raise BlueprintV2Error("v2 review draft must explicitly set authoritative: false")
    declared_states = str(header.get("status_values", ""))
    expected_states = "AUDITED|PARTIAL|PLANNED|DEFERRED|ACCEPTED"
    if declared_states != expected_states:
        raise BlueprintV2Error(
            "status_values must be AUDITED|PARTIAL|PLANNED|DEFERRED|ACCEPTED"
        )
    version = str(header.get("blueprint_version", ""))
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", version):
        raise BlueprintV2Error("blueprint_version must be semantic x.y.z")
    if "per_item_code_loc_cap" not in header:
        raise BlueprintV2Error("missing per_item_code_loc_cap")
    cap = header["per_item_code_loc_cap"]
    # The product contract is an exclusive 5,000-line ceiling per item.  A
    # draft cannot weaken that invariant by declaring a larger local cap.
    if cap != DEFAULT_CAP:
        raise BlueprintV2Error(
            f"per_item_code_loc_cap must remain exactly {DEFAULT_CAP} (exclusive)"
        )
    rows = _rows(text)
    for item_id, (_, _, loc, line_number) in rows.items():
        if loc < 0 or loc >= cap:
            raise BlueprintV2Error(f"line {line_number}: {item_id} LOC {loc} is not < {cap}")
    _check_cycles(rows)
    counts = {state: sum(row[0] == state for row in rows.values()) for state in sorted(STATES)}
    return {
        "ok": True,
        "path": path.as_posix(),
        "schema_version": header["schema_version"],
        "blueprint_version": version,
        "per_item_code_loc_cap": cap,
        "rows": len(rows),
        "states": counts,
        "max_estimated_loc": max(row[2] for row in rows.values()),
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Validate the zenpi v2 Blueprint review draft")
    parser.add_argument("path", nargs="?", type=Path, default=DEFAULT_PATH)
    parser.add_argument("--json", action="store_true", help="emit machine-readable output")
    args = parser.parse_args(argv)
    try:
        report = validate(args.path)
    except BlueprintV2Error as exc:
        report = {"ok": False, "path": args.path.as_posix(), "error": str(exc)}
    if args.json:
        print(json.dumps(report, sort_keys=True))
    elif report["ok"]:
        print(
            f"Blueprint v2 valid: {report['rows']} rows, "
            f"max LOC {report['max_estimated_loc']} < {report['per_item_code_loc_cap']}"
        )
    else:
        print(f"ERROR: {report['error']}", file=sys.stderr)
    return 0 if report["ok"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
