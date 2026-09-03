#!/usr/bin/env python3
"""Check zenpi's physical Rust source line budget.

The checker deliberately counts physical lines, rather than attempting to
classify comments, whitespace, or generated-looking source.  This makes the
budget deterministic and cheap to run in a clean checkout.  Only ``.rs``
files below the four source roots are considered:

    src/ tests/ examples/ benches/

Vendored subdirectories are excluded even when nested below one of those
roots.  Build output remains excluded because it is outside the source roots.

The human-readable form is intended for local use; ``--json`` is suitable for
CI and execution-controller validators.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable


DEFAULT_LIMIT = 5_000
SOURCE_ROOTS = ("src", "tests", "examples", "benches")
EXCLUDED_DIRECTORIES = {"target", "vendor", "vendored"}
EXIT_OK = 0
EXIT_EXCEEDED = 1
EXIT_USAGE = 2


@dataclass(frozen=True)
class RustFile:
    """A counted Rust file and its physical line count."""

    path: str
    lines: int
    root: str


class CheckError(Exception):
    """An expected filesystem or command-line validation error."""


def _physical_line_count(data: bytes) -> int:
    """Return physical LF-delimited lines, including a non-terminated tail."""

    if not data:
        return 0
    return data.count(b"\n") + (0 if data.endswith(b"\n") else 1)


def _rust_files(root: Path) -> Iterable[RustFile]:
    """Yield all regular ``.rs`` files below source roots in stable order.

    ``os.walk`` does not follow directory symlinks.  Symlinked files are
    skipped too, avoiding accidental counting of files outside the checkout
    and making repeated invocations independent of symlink targets.
    """

    for source_root in SOURCE_ROOTS:
        directory = root / source_root
        if not directory.exists():
            continue
        if directory.is_symlink():
            continue
        if not directory.is_dir():
            raise CheckError(f"source root is not a directory: {source_root}")

        for current, dirnames, filenames in os.walk(
            directory, topdown=True, followlinks=False
        ):
            # Do not descend through symlinked directories even on platforms
            # where os.walk reports them in dirnames.
            dirnames[:] = sorted(
                name
                for name in dirnames
                if name not in EXCLUDED_DIRECTORIES
                and not (Path(current) / name).is_symlink()
            )
            for name in sorted(filenames):
                if not name.endswith(".rs"):
                    continue
                candidate = Path(current) / name
                if candidate.is_symlink() or not candidate.is_file():
                    continue
                try:
                    data = candidate.read_bytes()
                except OSError as exc:
                    relative = candidate.relative_to(root).as_posix()
                    raise CheckError(f"cannot read {relative}: {exc}") from exc
                relative = candidate.relative_to(root).as_posix()
                yield RustFile(
                    path=relative,
                    lines=_physical_line_count(data),
                    root=source_root,
                )


def check(root: Path, limit: int) -> dict[str, object]:
    """Build a serializable line-budget report for ``root``."""

    files = list(_rust_files(root))
    total = sum(item.lines for item in files)
    by_root = {source_root: 0 for source_root in SOURCE_ROOTS}
    file_counts = {source_root: 0 for source_root in SOURCE_ROOTS}
    for item in files:
        by_root[item.root] += item.lines
        file_counts[item.root] += 1

    return {
        "schema_version": "rust-loc/v1",
        "root": str(root),
        "roots": list(SOURCE_ROOTS),
        "max_lines": limit,
        # line_budget is retained as a descriptive alias for policy tooling.
        "line_budget": limit,
        "total_lines": total,
        "remaining_lines": limit - total,
        "exceeded": total > limit,
        "within_budget": total <= limit,
        "file_count": len(files),
        "lines_by_root": by_root,
        "files_by_root": file_counts,
        "files": [
            {"path": item.path, "root": item.root, "lines": item.lines}
            for item in files
        ],
    }


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description=(
            "Count physical lines in .rs files under src, tests, examples, "
            "and benches."
        )
    )
    parser.add_argument(
        "root",
        nargs="?",
        default=".",
        help="repository root (default: current directory)",
    )
    parser.add_argument(
        "--root",
        dest="root_option",
        metavar="PATH",
        help="repository root (alternative to the positional argument)",
    )
    parser.add_argument(
        "--max-lines",
        "--limit",
        dest="limit",
        default=DEFAULT_LIMIT,
        type=int,
        metavar="N",
        help=f"maximum allowed physical lines (default: {DEFAULT_LIMIT})",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="emit the complete machine-readable report as JSON",
    )
    return parser


def main(argv: list[str] | None = None) -> int:
    parser = _parser()
    args = parser.parse_args(argv)
    if args.limit < 0:
        parser.error("--max-lines must be non-negative")

    try:
        root = Path(args.root_option or args.root).expanduser().resolve(strict=True)
        if not root.is_dir():
            raise CheckError(f"root is not a directory: {root}")
        report = check(root, args.limit)
    except (CheckError, OSError, RuntimeError) as exc:
        if args.json:
            print(
                json.dumps(
                    {
                        "schema_version": "rust-loc/v1",
                        "error": str(exc),
                        "within_budget": False,
                    },
                    sort_keys=True,
                )
            )
        else:
            print(f"check_rust_loc: error: {exc}", file=sys.stderr)
        return EXIT_USAGE

    if args.json:
        print(json.dumps(report, sort_keys=True))
    else:
        state = "PASS" if report["within_budget"] else "FAIL"
        print(
            f"{state}: {report['total_lines']} / {report['max_lines']} Rust "
            f"physical lines across {report['file_count']} files"
        )
        for source_root in SOURCE_ROOTS:
            print(
                f"  {source_root}: {report['lines_by_root'][source_root]} "
                f"lines ({report['files_by_root'][source_root]} files)"
            )
        if not report["within_budget"]:
            print(
                f"  over budget by {report['total_lines'] - report['max_lines']} "
                "lines",
                file=sys.stderr,
            )

    return EXIT_OK if report["within_budget"] else EXIT_EXCEEDED


if __name__ == "__main__":
    raise SystemExit(main())
