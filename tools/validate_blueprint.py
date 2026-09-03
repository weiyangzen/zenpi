#!/usr/bin/env python3
"""Validate zenpi's authoritative execution blueprint and its Gantt projection.

This intentionally uses only the Python standard library.  The repository's
blueprint has a small, frozen YAML header and a deliberately regular Markdown
row format; accepting that format directly avoids making PyYAML a runtime
dependency of the project being validated.
"""

from __future__ import annotations

import argparse
import datetime as _datetime
import hashlib
import json
import re
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable


ROOT = Path(__file__).resolve().parents[1]
BLUEPRINT_REL = Path("Docs/Zenpi_Execution_Blueprint.md")
SPEC_REL = Path("Docs/Zenpi_Execution_Spec.md")
GANTT_REL = Path("Docs/Zenpi_Execution_Gantt.md")
STATUS_TO_STATE = {" ": "unclaimed", "_": "self_tested", "x": "master_accepted"}
HEX_SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
EXCLUDED_RUST_DIRECTORIES = {"target", "vendor", "vendored"}


@dataclass(frozen=True)
class Item:
    item_id: str
    mark: str
    layer: str
    depends: tuple[str, ...]
    owner: str
    owned_paths: tuple[str, ...]
    estimated_loc: int
    line: int


class BlueprintError(Exception):
    """A validation failure with a stable, human-readable message."""


def _scalar(value: str) -> Any:
    """Parse the limited scalar subset used by the frozen YAML header."""
    value = value.strip()
    if not value:
        return ""
    if (value.startswith("'") and value.endswith("'")) or (
        value.startswith('"') and value.endswith('"')
    ):
        return value[1:-1]
    if value.lower() in {"true", "false"}:
        return value.lower() == "true"
    if re.fullmatch(r"-?\d+", value):
        return int(value)
    if value.startswith("[") and value.endswith("]"):
        body = value[1:-1].strip()
        return [] if not body else [_scalar(part) for part in body.split(",")]
    return value


def _yaml_header(text: str) -> dict[str, Any]:
    blocks = re.findall(r"```yaml\s*\n(.*?)\n```", text, re.DOTALL)
    if not blocks:
        raise BlueprintError("missing fenced yaml header")
    result: dict[str, Any] = {}
    for raw_line in blocks[0].splitlines():
        line = raw_line.strip()
        if not line or line.startswith("#"):
            continue
        match = re.fullmatch(r"([A-Za-z_][A-Za-z0-9_]*):\s*(.*)", line)
        if not match:
            raise BlueprintError(f"invalid yaml header line: {raw_line!r}")
        key, value = match.groups()
        if key in result:
            raise BlueprintError(f"duplicate yaml key: {key}")
        result[key] = _scalar(value)
    return result


def _field(row: str, name: str, next_names: Iterable[str]) -> str:
    names = "|".join(re.escape(item) for item in next_names)
    boundary = rf"(?=\s*\|\s*(?:{names}):|$)" if names else r"(?=$)"
    match = re.search(rf"\b{re.escape(name)}:\s*(.*?){boundary}", row)
    return match.group(1).strip() if match else ""


def _parse_items(text: str, stable_pattern: str) -> list[Item]:
    items: list[Item] = []
    stable_re = re.compile(stable_pattern)
    row_start = re.compile(r"^\s*-\s*\[([ _x])\]")
    checklist_start = re.compile(r"^\s*-\s*\[[^\]]*\]")
    id_re = re.compile(r"\*\*([^*]+)\*\*")
    for number, line in enumerate(text.splitlines(), 1):
        status = row_start.match(line)
        if not status:
            if checklist_start.match(line):
                raise BlueprintError(f"line {number}: unsupported checklist mark")
            continue
        ids = id_re.findall(line)
        if not ids:
            raise BlueprintError(f"line {number}: checklist row has no bold stable ID")
        item_id = ids[0]
        if not stable_re.fullmatch(item_id):
            raise BlueprintError(f"line {number}: invalid stable ID {item_id!r}")
        layer_match = re.search(r"\blayer\s+`([^`]+)`", line)
        layer = layer_match.group(1).strip() if layer_match else ""
        if not layer:
            raise BlueprintError(f"line {number}: {item_id} has no layer")
        depends_raw = _field(
            line,
            "Depends",
            ("Owner scope", "Owned paths", "Validators", "Rollback", "Estimate"),
        )
        if not depends_raw or depends_raw in {"\u2014", "-", "none", "None", "[]"}:
            depends: tuple[str, ...] = ()
        else:
            depends = tuple(part.strip() for part in depends_raw.split(",") if part.strip())
            if not all(stable_re.fullmatch(dep) for dep in depends):
                raise BlueprintError(f"line {number}: {item_id} has malformed dependency list")
            if len(set(depends)) != len(depends):
                raise BlueprintError(f"line {number}: {item_id} repeats a dependency")
        owner = _field(
            line,
            "Owner scope",
            ("Owned paths", "Validators", "Rollback", "Estimate"),
        )
        if not owner:
            raise BlueprintError(f"line {number}: {item_id} has no owner scope")
        paths_raw = _field(line, "Owned paths", ("Validators", "Rollback", "Estimate"))
        if not paths_raw:
            raise BlueprintError(f"line {number}: {item_id} has no owned paths")
        paths = tuple(re.findall(r"`([^`]+)`", paths_raw))
        if not paths:
            raise BlueprintError(f"line {number}: {item_id} has no repository-relative owned path")
        for path in paths:
            candidate = Path(path)
            if candidate.is_absolute() or ".." in candidate.parts:
                raise BlueprintError(f"line {number}: {item_id} owns unsafe path {path!r}")
            if not path or path.startswith(("./", "~/")):
                raise BlueprintError(f"line {number}: {item_id} owns malformed path {path!r}")
        validators = _field(line, "Validators", ("Rollback", "Estimate"))
        rollback = _field(line, "Rollback", ("Estimate",))
        estimate = _field(line, "Estimate", ("Estimated LOC",))
        if not validators or not rollback or not estimate:
            raise BlueprintError(f"line {number}: {item_id} is missing validator, rollback, or estimate")
        loc_values = re.findall(r"\|\s*Estimated LOC:\s*([^|]+?)(?=\s*\||$)", line)
        if len(loc_values) != 1:
            raise BlueprintError(f"line {number}: {item_id} must have exactly one Estimated LOC field")
        loc_raw = loc_values[0].strip()
        if not re.fullmatch(r"[0-9]+", loc_raw):
            raise BlueprintError(f"line {number}: {item_id} has a missing or non-integer Estimated LOC")
        items.append(Item(item_id, status.group(1), layer, depends, owner, paths, int(loc_raw), number))
    if not items:
        raise BlueprintError("no checklist rows found")
    return items


def _check_cycles(items: list[Item]) -> None:
    by_id = {item.item_id: item for item in items}
    for item in items:
        for dep in item.depends:
            if dep not in by_id:
                raise BlueprintError(f"{item.item_id} depends on missing ID {dep}")
    visiting: set[str] = set()
    visited: set[str] = set()

    def visit(item_id: str) -> None:
        if item_id in visiting:
            raise BlueprintError(f"dependency cycle detected at {item_id}")
        if item_id in visited:
            return
        visiting.add(item_id)
        for dep in by_id[item_id].depends:
            visit(dep)
        visiting.remove(item_id)
        visited.add(item_id)

    for item in items:
        visit(item.item_id)


def _check_dependency_states(items: list[Item]) -> None:
    """Ensure provisional or accepted work never closes before its parents."""
    by_id = {item.item_id: item for item in items}
    for item in items:
        if item.mark == " ":
            continue
        unfinished = [dep for dep in item.depends if by_id[dep].mark != "x"]
        if unfinished:
            raise BlueprintError(
                f"dependency state violation: {item.item_id} is {item.mark!r} "
                f"before accepted dependencies {', '.join(unfinished)}"
            )


def _digest(path: Path) -> str:
    try:
        return hashlib.sha256(path.read_bytes()).hexdigest()
    except OSError as exc:
        raise BlueprintError(f"cannot read {path}: {exc}") from exc


def _rust_line_count(root: Path) -> int:
    total = 0
    for directory in ("src", "tests", "examples", "benches"):
        base = root / directory
        if not base.is_dir():
            continue
        for path in base.rglob("*.rs"):
            if (
                path.is_file()
                and not path.is_symlink()
                and not EXCLUDED_RUST_DIRECTORIES.intersection(path.parts)
            ):
                try:
                    data = path.read_bytes()
                    total += data.count(b"\n") + (0 if not data or data.endswith(b"\n") else 1)
                except (OSError, UnicodeError) as exc:
                    raise BlueprintError(f"cannot count Rust source {path}: {exc}") from exc
    return total


def _required_header(header: dict[str, Any], root: Path) -> None:
    """Validate the frozen contract without baking in one checkout path.

    The source document records an absolute ``repository_root`` for operator
    identity.  A clone or test fixture necessarily has a different prefix, so
    the value is checked only for presence; all executable paths remain
    relative and are checked exactly.
    """
    required: dict[str, Any] = {
        "schema_version": "execution-blueprint/v1",
        "authoritative": True,
        "spec_path": str(SPEC_REL),
        "gantt_path": str(GANTT_REL),
        "status_marks": "[ ]|[_]|[x]",
        "stable_id_pattern": r"^ZP-[0-9]{3}$",
        "product_modes": "tui, headless only",
        "worker_lifecycle": "bounded",
        "nested_agents": "forbidden",
    }
    required.update(
        {
            "per_item_code_loc_cap": 5000,
            "loc_basis": "per-item forecast of implementation/test code LOC attributable to the row (Rust/Python/Shell); docs/config/generated artifacts count 0; declared forecast has a strict upper bound of 5000 (exclusive); aggregate repository LOC is informational",
        }
    )
    unknown = sorted(set(header) - set(required) - {"repository_root", "completion_policy"})
    if unknown:
        raise BlueprintError(f"unexpected yaml header keys: {', '.join(unknown)}")
    for key, value in required.items():
        if header.get(key) != value:
            raise BlueprintError(f"yaml header {key!r} must be {value!r}, got {header.get(key)!r}")

    declared_root = header.get("repository_root")
    if not isinstance(declared_root, str) or not declared_root.strip():
        raise BlueprintError("yaml header 'repository_root' must be a non-empty path")
    # Absolute paths are intentionally portable across clones and temporary
    # fixture roots.  The field is retained for operator identity, while all
    # executable source/spec paths below are checked relative to ``root``.

    policy = header.get("completion_policy")
    if not isinstance(policy, str) or "[x]" not in policy or "[_]" not in policy:
        raise BlueprintError("yaml header completion_policy must describe [x] acceptance and [_] handoff")

    cap = header.get("per_item_code_loc_cap")
    if not isinstance(cap, int) or cap != 5000:
        raise BlueprintError("yaml header per_item_code_loc_cap must be integer 5000")

    try:
        re.compile(str(header["stable_id_pattern"]))
    except re.error as exc:
        raise BlueprintError("yaml header stable_id_pattern is not a valid regex") from exc


def _gantt_name(blueprint_name: str) -> str:
    """Map only a terminal ``Blueprint`` token to ``Gantt``."""
    stem, dot, suffix = blueprint_name.rpartition(".")
    if not dot:
        stem, suffix = blueprint_name, ""
    if stem.endswith("Blueprint"):
        mapped = stem[: -len("Blueprint")] + "Gantt"
    else:
        mapped = stem + "_Gantt"
    return f"{mapped}.{suffix}" if suffix else mapped


def _monitoring_section(text: str) -> str:
    """Return only the monitoring-index section, excluding unrelated tables."""
    match = re.search(r"^##\s+Monitoring index\s*$", text, re.MULTILINE | re.IGNORECASE)
    if not match:
        raise BlueprintError("Gantt has no Monitoring index section")
    tail = text[match.end() :]
    next_heading = re.search(r"^##\s+", tail, re.MULTILINE)
    return tail[: next_heading.start()] if next_heading else tail


def _gantt_rows(text: str) -> list[tuple[str, str, tuple[str, ...]]]:
    rows: list[tuple[str, str, tuple[str, ...]]] = []
    section = _monitoring_section(text)
    header_seen = False
    for line in section.splitlines():
        if not line.lstrip().startswith("|"):
            continue
        cells = [cell.strip() for cell in line.strip().strip("|").split("|")]
        if cells and cells[0].lower() == "id":
            required_columns = {
                "id",
                "state",
                "depends on",
                "claim/owner",
                "startup/live",
                "handoff/integration/repair",
                "scheduling note",
            }
            if {cell.lower() for cell in cells} < required_columns:
                raise BlueprintError("Gantt monitoring index is missing required columns")
            header_seen = True
            continue
        if len(cells) < 3 or cells[0] in {"---", ""}:
            continue
        item_id = cells[0]
        if not re.fullmatch(r"[A-Za-z][A-Za-z0-9_.-]*", item_id):
            continue
        if len(cells) < 7:
            raise BlueprintError(f"Gantt monitoring row {item_id} is missing runtime columns")
        if any(not cells[index] for index in (1, 3, 4, 5, 6)):
            raise BlueprintError(f"Gantt monitoring row {item_id} has an empty runtime field")
        state = cells[1]
        raw_depends = cells[2].strip()
        if raw_depends in {"", "-", "\u2014", "none", "None"}:
            depends = ()
        else:
            depends = tuple(part for part in re.split(r"[, ]+", raw_depends) if part)
        rows.append((item_id, state, depends))
    if not header_seen:
        raise BlueprintError("Gantt monitoring index has no complete table header")
    return rows


def _spec_header(text: str) -> dict[str, Any]:
    """Parse and validate the first YAML block in the execution spec."""
    return _yaml_header(text)


def _require_spec(spec_header: dict[str, Any]) -> None:
    if spec_header.get("schema_version") != "execution-spec/v1":
        raise BlueprintError("execution specification has no execution-spec/v1 header")
    if spec_header.get("authoritative_blueprint") != str(BLUEPRINT_REL):
        raise BlueprintError("execution specification points at the wrong Blueprint")
    if spec_header.get("gantt_projection") != str(GANTT_REL):
        raise BlueprintError("execution specification points at the wrong Gantt projection")
    if spec_header.get("gantt_naming") != "exact-prefix-Blueprint-to-Gantt":
        raise BlueprintError("execution specification has no exact Blueprint-to-Gantt naming policy")
    if spec_header.get("worker_transport") != "tmux_codex_tui":
        raise BlueprintError("execution specification must freeze tmux_codex_tui transport")
    if spec_header.get("app_server_workers") != "forbidden":
        raise BlueprintError("execution specification must forbid app-server workers")
    if spec_header.get("nested_agents") != "forbidden":
        raise BlueprintError("execution specification must forbid nested agents")
    if spec_header.get("per_item_code_loc_cap") != 5000:
        raise BlueprintError("execution specification must freeze per-item code LOC cap at 5000")
    if spec_header.get("loc_policy") != "every Blueprint item has an integer Estimated LOC with 0 <= value < 5000; scope is a per-item forecast of implementation/test code attributable to the row (Rust/Python/Shell), not a current-file inventory; docs/config/generated artifacts count 0; the declared forecast has a strict upper bound of 5000 (exclusive); aggregate repository LOC is informational":
        raise BlueprintError("execution specification must describe strict per-item LOC semantics")
    modes = spec_header.get("product_modes")
    if isinstance(modes, list):
        normalized = [str(mode).strip().lower() for mode in modes]
    elif isinstance(modes, str):
        normalized = [part.strip().lower() for part in modes.strip("[]").split(",") if part.strip()]
    else:
        normalized = []
    if normalized != ["tui", "headless"]:
        raise BlueprintError("execution specification must freeze exactly [tui, headless] modes")


def _check_item_loc(items: list[Item], cap: int) -> None:
    for item in items:
        if not 0 <= item.estimated_loc < cap:
            raise BlueprintError(
                f"{item.item_id} Estimated LOC must satisfy 0 <= value < {cap}, got {item.estimated_loc}"
            )


def _normalize_state(value: str) -> str:
    """Normalize harmless display spelling differences in Gantt states."""
    normalized = value.strip().lower().replace("-", "_").replace(" ", "_")
    aliases = {
        "open": "unclaimed",
        "pending": "unclaimed",
        "selftested": "self_tested",
        "self_tested_handoff": "self_tested",
        "accepted": "master_accepted",
        "masteraccepted": "master_accepted",
        "complete": "master_accepted",
    }
    return aliases.get(normalized, normalized)


def validate(root: Path = ROOT) -> dict[str, Any]:
    root = root.resolve()
    blueprint = root / BLUEPRINT_REL
    spec = root / SPEC_REL
    gantt = root / GANTT_REL
    errors: list[str] = []
    docs_dir = root / BLUEPRINT_REL.parent
    if docs_dir.is_dir():
        candidates = sorted(
            path.relative_to(root).as_posix()
            for path in docs_dir.rglob("*Blueprint*.md")
            if path.is_file() and not path.is_symlink()
        )
        if candidates != [BLUEPRINT_REL.as_posix()]:
            errors.append(
                "exactly one authoritative Blueprint is required; found "
                + (", ".join(candidates) if candidates else "none")
            )
    for path in (blueprint, spec, gantt):
        if not path.is_file():
            errors.append(f"missing required file: {path.relative_to(root)}")
    if errors:
        return {"ok": False, "errors": errors, "counts": {}}
    try:
        blueprint_text = blueprint.read_text(encoding="utf-8")
        spec_text = spec.read_text(encoding="utf-8")
        gantt_text = gantt.read_text(encoding="utf-8")
        header = _yaml_header(blueprint_text)
        _required_header(header, root)
        items = _parse_items(blueprint_text, str(header["stable_id_pattern"]))
        _check_item_loc(items, int(header["per_item_code_loc_cap"]))
        ids = [item.item_id for item in items]
        duplicates = sorted({item_id for item_id in ids if ids.count(item_id) > 1})
        if duplicates:
            raise BlueprintError(f"duplicate checklist IDs: {', '.join(duplicates)}")
        _check_cycles(items)
        _check_dependency_states(items)
        _require_spec(_spec_header(spec_text))
        gantt_header = _yaml_header(gantt_text)
        if gantt_header.get("schema_version") != "execution-gantt/v1":
            raise BlueprintError("Gantt has no execution-gantt/v1 header")
        if gantt_header.get("source_path") != str(BLUEPRINT_REL):
            raise BlueprintError("Gantt source_path does not point to the authoritative Blueprint")
        if gantt_header.get("spec_path") != str(SPEC_REL):
            raise BlueprintError("Gantt spec_path does not point to the frozen specification")
        if gantt_header.get("projection_authority") is not False:
            raise BlueprintError("Gantt projection_authority must be false")
        generated_at = gantt_header.get("generated_at")
        if not isinstance(generated_at, str):
            raise BlueprintError("Gantt generated_at must be an RFC3339 timestamp")
        try:
            parsed_timestamp = _datetime.datetime.fromisoformat(generated_at.replace("Z", "+00:00"))
        except ValueError as exc:
            raise BlueprintError("Gantt generated_at must be an RFC3339 timestamp") from exc
        if parsed_timestamp.tzinfo is None:
            raise BlueprintError("Gantt generated_at must include a timezone")
        expected_gantt_name = _gantt_name(blueprint.name)
        if gantt.name != expected_gantt_name:
            raise BlueprintError(f"Gantt filename must be {expected_gantt_name}")
        source_sha = _digest(blueprint)
        spec_sha = _digest(spec)
        gantt_source_sha = gantt_header.get("source_sha256")
        gantt_spec_sha = gantt_header.get("spec_sha256")
        if not isinstance(gantt_source_sha, str) or not HEX_SHA256_RE.fullmatch(gantt_source_sha):
            raise BlueprintError("Gantt source_sha256 must be a 64-character lowercase SHA-256 digest")
        if not isinstance(gantt_spec_sha, str) or not HEX_SHA256_RE.fullmatch(gantt_spec_sha):
            raise BlueprintError("Gantt spec_sha256 must be a 64-character lowercase SHA-256 digest")
        if gantt_source_sha != source_sha:
            raise BlueprintError("Gantt source_sha256 is stale")
        if gantt_spec_sha != spec_sha:
            raise BlueprintError("Gantt spec_sha256 is stale")
        if re.search(r"\[[ _xX]\]", gantt_text):
            raise BlueprintError("Gantt contains a mutable checklist mark")
        mermaid = re.search(r"```mermaid\s*\n(.*?)\n```", gantt_text, re.DOTALL | re.IGNORECASE)
        if not mermaid or not re.search(r"^\s*gantt\s*$", mermaid.group(1), re.MULTILINE | re.IGNORECASE):
            raise BlueprintError("Gantt has no renderable Mermaid gantt view")
        gantt_rows = _gantt_rows(gantt_text)
        gantt_ids = [item_id for item_id, _, _ in gantt_rows]
        if sorted(gantt_ids) != sorted(ids):
            missing = sorted(set(ids) - set(gantt_ids))
            extra = sorted(set(gantt_ids) - set(ids))
            raise BlueprintError(f"Gantt monitoring index mismatch (missing={missing}, extra={extra})")
        gantt_duplicates = sorted({item_id for item_id in gantt_ids if gantt_ids.count(item_id) > 1})
        if gantt_duplicates:
            raise BlueprintError(f"Gantt duplicates IDs: {', '.join(gantt_duplicates)}")
        gantt_by_id = {item_id: (state, depends) for item_id, state, depends in gantt_rows}
        for item in items:
            expected_state = STATUS_TO_STATE[item.mark]
            state, gantt_depends = gantt_by_id[item.item_id]
            if _normalize_state(state) != expected_state:
                raise BlueprintError(f"Gantt state for {item.item_id} is {state!r}, expected {expected_state!r}")
            if set(gantt_depends) != set(item.depends):
                raise BlueprintError(
                    f"Gantt dependencies for {item.item_id} do not match Blueprint "
                    f"(got={list(gantt_depends)}, expected={list(item.depends)})"
                )
        counts = {
            "unclaimed": sum(item.mark == " " for item in items),
            "self_tested": sum(item.mark == "_" for item in items),
            "master_accepted": sum(item.mark == "x" for item in items),
        }
        summary = tuple(gantt_header.get(key) for key in ("unclaimed", "self_tested", "master_accepted"))
        expected_summary = (counts["unclaimed"], counts["self_tested"], counts["master_accepted"])
        if summary != expected_summary:
            raise BlueprintError("Gantt state_summary does not match Blueprint marks")
        rust_lines = _rust_line_count(root)
        return {
            "ok": True,
            "errors": [],
            "counts": counts,
            "items": len(items),
            "checklist_ids": ids,
            "dependency_edges": {
                item.item_id: list(item.depends) for item in items if item.depends
            },
            "blueprint_path": BLUEPRINT_REL.as_posix(),
            "gantt_path": GANTT_REL.as_posix(),
            "aggregate_rust_source_lines_informational": rust_lines,
            "estimated_loc_by_id": {item.item_id: item.estimated_loc for item in items},
            "max_estimated_loc": max(item.estimated_loc for item in items),
            "source_sha256": source_sha,
            "spec_sha256": spec_sha,
        }
    except (BlueprintError, OSError, UnicodeError) as exc:
        return {"ok": False, "errors": [str(exc)], "counts": {}}


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root_positional", nargs="?", type=Path, help="zenpi repository root")
    parser.add_argument("--root", dest="root_option", type=Path, help="zenpi repository root")
    parser.add_argument("--json", action="store_true", help="emit a machine-readable report")
    args = parser.parse_args(argv)
    if args.root_positional is not None and args.root_option is not None:
        parser.error("specify the repository root once (positional or --root)")
    report = validate(args.root_option or args.root_positional or ROOT)
    if args.json:
        print(json.dumps(report, sort_keys=True))
    elif report["ok"]:
        counts = report["counts"]
        print(
            "Blueprint valid: "
            f"{report['items']} items, "
            f"{counts['unclaimed']} unclaimed, "
            f"{counts['self_tested']} self-tested, "
            f"{counts['master_accepted']} master-accepted"
        )
    else:
        for error in report["errors"]:
            print(f"ERROR: {error}", file=sys.stderr)
    return 0 if report["ok"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
