#!/usr/bin/env python3
"""Reproducible zenpi size/startup/RSS/runtime/render/layout budget probe.

This is a regression gate, not a statistical latency benchmark.  Every timed
case reports all raw samples plus min/median/max; it deliberately makes no p95
claim.  Run from the repository root after installing the declared Rust MSRV.
"""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import platform
import statistics
import subprocess
import sys
import tempfile
import time
from typing import Any


ROOT = pathlib.Path(__file__).resolve().parents[1]

BUDGETS = {
    "normal_dependencies_max": 16,
    "release_binary_bytes_max": 8 * 1024 * 1024,
    "cold_start_max_ms_max": 1000.0,
    "cold_start_peak_rss_max_bytes": 96 * 1024 * 1024,
    "queue_round_trip_max_us": 2000.0,
    "render_max_us": 10000.0,
    "layout_max_us": 100.0,
    "coalesced_frames_max": 1,
}


def run(command: list[str], **kwargs: Any) -> subprocess.CompletedProcess[str]:
    return subprocess.run(command, cwd=ROOT, text=True, check=True, **kwargs)


def summary(samples: list[float]) -> dict[str, Any]:
    return {
        "samples": samples,
        "min": min(samples),
        "median": statistics.median(samples),
        "max": max(samples),
        "sample_count": len(samples),
    }


def timed_process(
    command: list[str], *, env: dict[str, str], payload: str
) -> tuple[subprocess.CompletedProcess[str], int]:
    """Run one binary under the platform time(1) RSS collector."""
    if sys.platform == "darwin":
        wrapped = ["/usr/bin/time", "-l", *command]
        marker = "maximum resident set size"
        multiplier = 1
    elif sys.platform.startswith("linux"):
        wrapped = ["/usr/bin/time", "-v", *command]
        marker = "Maximum resident set size (kbytes)"
        multiplier = 1024
    else:
        raise RuntimeError("RSS gate currently supports Darwin and Linux")
    process = subprocess.run(
        wrapped, cwd=ROOT, env=env, input=payload, text=True,
        capture_output=True, timeout=5,
    )
    rss = None
    for line in process.stderr.splitlines():
        if marker in line:
            fields = line.replace(":", " ").split()
            numbers = [int(field) for field in fields if field.isdigit()]
            if numbers:
                rss = numbers[-1] * multiplier
                break
    if rss is None:
        raise RuntimeError(f"could not parse peak RSS from time(1): {process.stderr!r}")
    # Keep diagnostics from the measured child separate from the RSS wrapper.
    # Normal benchmark exits are silent apart from time(1), but a failure still
    # returns the complete stderr below for diagnosis.
    return process, rss


def dependency_receipt() -> dict[str, Any]:
    metadata = json.loads(run(["cargo", "metadata", "--format-version", "1", "--no-deps", "--locked"], capture_output=True).stdout)
    package = next(item for item in metadata["packages"] if item["name"] == "zenpi")
    normal = sorted(
        {item["name"] for item in package["dependencies"] if item.get("kind") in (None, "normal")}
    )
    tree = run(["cargo", "tree", "--depth", "1", "--locked"], capture_output=True).stdout.splitlines()
    return {"normal_count": len(normal), "normal": normal, "features": package["features"], "tree": tree}


def build_and_measure_startup(samples: int) -> tuple[pathlib.Path, dict[str, Any]]:
    run(["cargo", "build", "--release", "--locked"], stdout=subprocess.DEVNULL)
    binary = ROOT / "target" / "release" / "zenpi"
    elapsed: list[float] = []
    rss: list[float] = []
    with tempfile.TemporaryDirectory(prefix="zenpi-bench-") as home:
        env = os.environ.copy()
        env.update({
            "HOME": home,
            "ZENPI_HOME": str(pathlib.Path(home) / "zenpi"),
            "ZENPI_BACKEND": "openai",
            "ZENPI_BASE_URL": "http://127.0.0.1:1",
            "ZENPI_MODEL": "benchmark-no-network",
            "ZENPI_API_KEY": "benchmark-placeholder",
        })
        payload = '{"type":"shutdown","id":"bench-stop"}\n'
        for index in range(samples):
            session = pathlib.Path(home) / f"session-{index}.jsonl"
            started = time.perf_counter_ns()
            process, peak_rss = timed_process(
                [str(binary), "--mode", "headless", "--session", str(session)],
                env=env, payload=payload,
            )
            elapsed.append((time.perf_counter_ns() - started) / 1_000_000)
            if process.returncode != 0 or '"success":true' not in process.stdout:
                raise RuntimeError(f"cold startup failed: rc={process.returncode} stderr={process.stderr!r}")
            rss.append(float(peak_rss))
    return binary, {
        "elapsed_ms": summary(elapsed),
        "process_peak_rss_bytes": summary(rss),
    }


def run_probe() -> dict[str, Any]:
    output = subprocess.run(
        [
            "cargo",
            "run",
            "--release",
            "--quiet",
            "--example",
            "runtime_budget_probe",
            "--locked",
        ],
        cwd=ROOT, text=True, capture_output=True, check=True, timeout=180,
    )
    return json.loads(output.stdout)


def evaluate(report: dict[str, Any]) -> dict[str, bool]:
    elapsed = report["cold_start"]["elapsed_ms"]
    rss = report["cold_start"]["process_peak_rss_bytes"]
    probe = report["runtime_probe"]
    return {
        "normal_dependencies": report["dependencies"]["normal_count"] <= BUDGETS["normal_dependencies_max"],
        "release_binary": report["release_binary_bytes"] <= BUDGETS["release_binary_bytes_max"],
        "cold_start": elapsed["max"] <= BUDGETS["cold_start_max_ms_max"],
        "cold_start_peak_rss": rss["max"] <= BUDGETS["cold_start_peak_rss_max_bytes"],
        "queue_round_trip": probe["per_operation_us"]["queue_round_trip"] <= BUDGETS["queue_round_trip_max_us"],
        "render": probe["per_operation_us"]["render"] <= BUDGETS["render_max_us"],
        "layout": probe["per_operation_us"]["layout"] <= BUDGETS["layout_max_us"],
        "render_coalescing": probe["scheduler_frames_after_10000_dirty_requests"] <= BUDGETS["coalesced_frames_max"],
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--samples", type=int, default=5, help="cold-start samples (3-50)")
    parser.add_argument("--output", type=pathlib.Path, help="also write the JSON receipt atomically")
    parser.add_argument("--no-fail", action="store_true", help="report budget failures without a nonzero exit")
    args = parser.parse_args()
    if not 3 <= args.samples <= 50:
        parser.error("--samples must be between 3 and 50")
    dependencies = dependency_receipt()
    binary, cold_start = build_and_measure_startup(args.samples)
    report: dict[str, Any] = {
        "schema_version": 1,
        "measurement_kind": "reproducible_regression_gate_not_p95",
        "host": {"system": platform.system(), "release": platform.release(), "machine": platform.machine(), "python": platform.python_version()},
        "budgets": BUDGETS,
        "dependencies": dependencies,
        "release_binary_bytes": binary.stat().st_size,
        "cold_start": cold_start,
        "runtime_probe": run_probe(),
    }
    report["gates"] = evaluate(report)
    report["ok"] = all(report["gates"].values())
    encoded = json.dumps(report, indent=2, sort_keys=True) + "\n"
    if args.output:
        destination = args.output if args.output.is_absolute() else ROOT / args.output
        destination.parent.mkdir(parents=True, exist_ok=True)
        temporary = destination.with_suffix(destination.suffix + ".tmp")
        temporary.write_text(encoded, encoding="utf-8")
        os.replace(temporary, destination)
    sys.stdout.write(encoded)
    return 0 if report["ok"] or args.no_fail else 1


if __name__ == "__main__":
    raise SystemExit(main())
