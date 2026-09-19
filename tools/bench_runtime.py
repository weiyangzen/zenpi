#!/usr/bin/env python3
"""Reproducible zenpi size/startup/RSS/runtime/render/layout budget probe.

This is a regression gate, not a statistical latency benchmark.  Every timed
case reports all raw samples plus min/median/max; it deliberately makes no p95
claim. Run from the repository root after installing the declared Rust MSRV.
On Apple Silicon macOS it selects the repository's stable ARM64 toolchain.
"""

from __future__ import annotations

import argparse
import base64
import datetime as dt
import hashlib
import json
import os
import pathlib
import platform
import statistics
import signal
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
    # Per-headless-process footprint ceilings (ZS1-162). Idle bounds a worker
    # blocked on stdin/provider I/O; busy bounds one executing a turn. CPU is a
    # percentage of one logical CPU, averaged since the worker started.
    "headless_idle_cpu_percent_max": 5.0,
    "headless_idle_rss_bytes_max": 32 * 1024 * 1024,
    "headless_busy_cpu_percent_max": 100.0,
    "headless_busy_rss_bytes_max": 512 * 1024 * 1024,
    "headless_footprint_processes_max": 64,
}

HEADLESS_FOOTPRINT_SCHEMA_VERSION = 1


def run(command: list[str], **kwargs: Any) -> subprocess.CompletedProcess[str]:
    # CI and developer machines may have a host-default toolchain that does
    # not match the checked-in macOS ARM build target. Keep the probe aligned
    # with the release commands used by this repository while preserving the
    # platform default elsewhere.
    if command and command[0] == "cargo" and platform.system() == "Darwin" and platform.machine() == "arm64":
        command = ["cargo", "+stable-aarch64-apple-darwin", *command[1:]]
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
    command: list[str], *, env: dict[str, str], payload: str,
    observation: dict[str, Any] | None = None, timeout: float = 5,
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
    observation = observation if observation is not None else {}
    observation.update(
        argv=wrapped, cwd=str(ROOT), started_at=dt.datetime.now(dt.timezone.utc).isoformat(),
        pid_scope='time(1) wrapper/process-group leader; not the zenpi child PID',
        phase_scope='parent observations, not internal product phase attribution',
    )
    started = time.perf_counter_ns()
    child = subprocess.Popen(
        wrapped, cwd=ROOT, env=env, stdin=subprocess.PIPE,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True,
    )
    spawned = time.perf_counter_ns()
    timed_out = False
    try:
        stdout, stderr = child.communicate(payload.encode(), timeout=timeout)
    except subprocess.TimeoutExpired:
        timed_out = True
        # time(1) has a child. Reap this isolated probe group, not only its wrapper.
        try:
            os.killpg(child.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        stdout, stderr = child.communicate()
    finished = time.perf_counter_ns()
    observation.update(
        wrapper_pid=child.pid, returncode=child.returncode, timed_out=timed_out,
        popen_return_ms=(spawned-started)/1_000_000,
        communicate_ms=(finished-spawned)/1_000_000,
        process_observed_ms=(finished-started)/1_000_000,
        ended_at=dt.datetime.now(dt.timezone.utc).isoformat(),
    )
    # Hold captured bytes until the outer gate timer stops. Encoding, hashing
    # and artifact writes are deliberately outside that timed interval.
    observation['_streams'] = {'stdout': stdout, 'stderr': stderr}
    process = subprocess.CompletedProcess(wrapped, child.returncode,
        stdout.decode('utf-8', errors='replace'), stderr.decode('utf-8', errors='replace'))
    if timed_out:
        raise subprocess.TimeoutExpired(wrapped, timeout, output=stdout, stderr=stderr)
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


def write_startup_observation(directory: pathlib.Path | None, index: int, observation: dict[str, Any]) -> None:
    for name, raw in observation.pop('_streams', {}).items():
        observation[name] = dict(
            bytes=len(raw), sha256=hashlib.sha256(raw).hexdigest(),
            prefix_base64=base64.b64encode(raw[:65536]).decode('ascii'),
            truncated=len(raw)>65536,
        )
    if directory is None:
        return
    directory.mkdir(parents=True, exist_ok=True)
    path = directory / f'sample-{index:02}.json'
    # Refuse to overwrite an earlier run's cold first sample.
    with path.open('x', encoding='utf-8') as handle:
        json.dump(observation, handle, ensure_ascii=False, indent=2)
        handle.write('\n')


def build_and_measure_startup(samples: int, evidence_dir: pathlib.Path | None = None) -> tuple[pathlib.Path, dict[str, Any]]:
    run(["cargo", "build", "--release", "--locked"], stdout=subprocess.DEVNULL)
    binary = ROOT / "target" / "release" / "zenpi"
    elapsed: list[float] = []
    rss: list[float] = []
    observations: list[dict[str, Any]] = []
    binary_hash = hashlib.sha256(binary.read_bytes()).hexdigest()
    with tempfile.TemporaryDirectory(prefix="zenpi-bench-") as fixture_root:
        env = os.environ.copy()
        env.update({
            "ZENPI_HOME": str(pathlib.Path(fixture_root) / "zenpi"),
            "ZENPI_BACKEND": "openai",
            "ZENPI_BASE_URL": "http://127.0.0.1:1",
            "ZENPI_MODEL": "benchmark-no-network",
            "ZENPI_API_KEY": "benchmark-placeholder",
        })
        payload = '{"type":"shutdown","id":"bench-stop"}\n'
        for index in range(samples):
            session = pathlib.Path(fixture_root) / f"session-{index}.jsonl"
            observation = dict(index=index, binary_sha256=binary_hash,
                               session=str(session), fresh_session=True)
            started = time.perf_counter_ns()
            try:
                process, peak_rss = timed_process(
                    [str(binary), "--mode", "headless", "--session", str(session)],
                    env=env, payload=payload, observation=observation,
                )
            except Exception as error:
                observation.update(elapsed_ms=(time.perf_counter_ns()-started)/1_000_000,
                                   error_type=type(error).__name__)
                write_startup_observation(evidence_dir, index, observation)
                raise
            elapsed.append((time.perf_counter_ns() - started) / 1_000_000)
            observation.update(elapsed_ms=elapsed[-1], peak_rss_bytes=peak_rss)
            write_startup_observation(evidence_dir, index, observation)
            observations.append(observation)
            if process.returncode != 0 or '"success":true' not in process.stdout:
                raise RuntimeError(f"cold startup failed: rc={process.returncode} stderr={process.stderr!r}")
            rss.append(float(peak_rss))
    return binary, {
        "elapsed_ms": summary(elapsed),
        "process_peak_rss_bytes": summary(rss),
        "binary_sha256": binary_hash,
        "observations": observations,
    }


def build_and_measure_near_cap(binary: pathlib.Path) -> dict[str, Any]:
    """Measure one valid journal close to the 256 MiB startup ceiling.

    This is evidence only, not a product budget gate: the session owner keeps
    validated records for replay, so the receipt records the real memory/time
    cost without pretending that a near-limit journal is an ordinary startup.
    """
    target = 240 * 1024 * 1024
    with tempfile.TemporaryDirectory(prefix="zenpi-bench-near-cap-") as fixture_root:
        fixture_path = pathlib.Path(fixture_root)
        session = fixture_path / "near-cap.jsonl"
        header = json.dumps({
            "kind": "session", "version": 1, "session_id": "bench-near-cap",
            "created_at_ms": 1, "cwd": str(ROOT),
        }, separators=(",", ":")) + "\n"
        event = json.dumps({
            "kind": "event", "event": {
                "type": "benchmark", "payload": "x" * 4096,
            },
        }, separators=(",", ":")) + "\n"
        with session.open("w", encoding="utf-8") as handle:
            handle.write(header)
            remaining = target - len(header)
            while remaining > len(event):
                handle.write(event)
                remaining -= len(event)
            handle.write(event[:max(0, remaining)])
        env = os.environ.copy()
        env.update({
            "ZENPI_HOME": str(fixture_path / "zenpi"),
            "ZENPI_BACKEND": "openai", "ZENPI_BASE_URL": "http://127.0.0.1:1",
            "ZENPI_MODEL": "benchmark-no-network", "ZENPI_API_KEY": "benchmark-placeholder",
        })
        started = time.perf_counter_ns()
        process, peak_rss = timed_process(
            [str(binary), "--mode", "headless", "--session", str(session)],
            env=env, payload='{"type":"shutdown","id":"bench-stop"}\n',
        )
        elapsed = (time.perf_counter_ns() - started) / 1_000_000
        if process.returncode != 0 or '"success":true' not in process.stdout:
            raise RuntimeError(
                f"near-cap startup failed: rc={process.returncode} stderr={process.stderr!r}"
            )
        return {
            "target_bytes": target,
            "journal_bytes": session.stat().st_size,
            "elapsed_ms": elapsed,
            "process_peak_rss_bytes": peak_rss,
        }


def parse_ps_duration_seconds(text: str) -> float | None:
    """Parse a `ps` duration (`MM:SS`, `HH:MM:SS`, `DD-HH:MM:SS`) into seconds."""
    days = 0
    clock = text
    if '-' in text:
        raw_days, _, clock = text.partition('-')
        try:
            days = int(raw_days)
        except ValueError:
            return None
    parts = clock.split(':')
    if not 1 <= len(parts) <= 3:
        return None
    try:
        values = [float(part) for part in parts]
    except ValueError:
        return None
    while len(values) < 3:
        values.insert(0, 0.0)
    hours, minutes, seconds = values
    return days * 86400.0 + hours * 3600.0 + minutes * 60.0 + seconds


def sample_process_footprint(pid: int) -> dict[str, Any] | None:
    """Read one process' RSS and average CPU from the platform `ps` owner."""
    output = subprocess.run(
        ["ps", "-o", "rss=,time=,etime=", "-p", str(pid)],
        text=True, capture_output=True,
    )
    if output.returncode != 0:
        return None
    fields = output.stdout.split()
    if len(fields) < 3:
        return None
    try:
        rss_kib = int(fields[0])
    except ValueError:
        return None
    cpu_seconds = parse_ps_duration_seconds(fields[1])
    elapsed_seconds = parse_ps_duration_seconds(fields[2])
    if cpu_seconds is None or elapsed_seconds is None:
        return None
    cpu_percent = 0.0 if elapsed_seconds <= 0 else (cpu_seconds / elapsed_seconds) * 100.0
    return {
        "pid": pid,
        "rss_bytes": rss_kib * 1024,
        "cpu_percent": cpu_percent,
        "elapsed_seconds": elapsed_seconds,
        "cpu_seconds": cpu_seconds,
    }


def footprint_gates(row: dict[str, Any], *, phase: str) -> dict[str, bool]:
    return {
        f"{phase}_rss": row["max_rss_bytes"] <= BUDGETS[f"headless_{phase}_rss_bytes_max"],
        f"{phase}_cpu": row["max_cpu_percent"] <= BUDGETS[f"headless_{phase}_cpu_percent_max"],
    }


def measure_headless_footprint(
    binary: pathlib.Path, *, workers: int = 1, samples: int = 5, interval: float = 0.2,
    timeout: float = 15,
) -> dict[str, Any]:
    """Measure idle per-process RSS/CPU for `workers` long-lived headless hosts.

    Each worker is started under the same no-network fixture as the startup
    gate, sampled while blocked on stdin, then asked to shut down cleanly. Rows
    are emitted per PID so the gate is a per-process check, not a host average.
    """
    if not 1 <= workers <= BUDGETS["headless_footprint_processes_max"]:
        raise ValueError("workers exceeds the headless_footprint_processes_max budget")
    with tempfile.TemporaryDirectory(prefix="zenpi-bench-footprint-") as fixture_root:
        env = os.environ.copy()
        env.update({
            "ZENPI_HOME": str(pathlib.Path(fixture_root) / "zenpi"),
            "ZENPI_BACKEND": "openai",
            "ZENPI_BASE_URL": "http://127.0.0.1:1",
            "ZENPI_MODEL": "benchmark-no-network",
            "ZENPI_API_KEY": "benchmark-placeholder",
        })
        processes = []
        sessions = []
        for index in range(workers):
            session = pathlib.Path(fixture_root) / f"footprint-{index}.jsonl"
            sessions.append(session)
            processes.append(subprocess.Popen(
                [str(binary), "--mode", "headless", "--session", str(session)],
                cwd=ROOT, env=env, stdin=subprocess.PIPE,
                stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            ))
        rows: list[dict[str, Any]] = []
        try:
            for index, process in enumerate(processes):
                samples_for_pid: list[dict[str, Any]] = []
                for _ in range(samples):
                    time.sleep(interval)
                    sample = sample_process_footprint(process.pid)
                    if sample is not None:
                        samples_for_pid.append(sample)
                if not samples_for_pid:
                    raise RuntimeError(f"could not sample headless worker pid={process.pid}")
                row = {
                    "pid": process.pid,
                    "session": str(sessions[index]),
                    "samples": samples_for_pid,
                    "max_rss_bytes": max(sample["rss_bytes"] for sample in samples_for_pid),
                    "max_cpu_percent": max(sample["cpu_percent"] for sample in samples_for_pid),
                    "phase": "idle",
                }
                row["gates"] = footprint_gates(row, phase="idle")
                row["ok"] = all(row["gates"].values())
                rows.append(row)
            for process in processes:
                process.stdin.write(b'{"type":"shutdown","id":"bench-footprint"}\n')
                process.stdin.flush()
            for process in processes:
                stdout, stderr = process.communicate(timeout=timeout)
                if process.returncode != 0 or b'"success":true' not in stdout:
                    raise RuntimeError(
                        f"headless footprint worker failed: rc={process.returncode} stderr={stderr!r}"
                    )
        finally:
            for process in processes:
                if process.poll() is None:
                    process.kill()
                    process.wait()
    return {
        "schema_version": HEADLESS_FOOTPRINT_SCHEMA_VERSION,
        "workers": workers,
        "samples_per_worker": samples,
        "rows": rows,
        "processes_within_cap": workers <= BUDGETS["headless_footprint_processes_max"],
        "ok": all(row["ok"] for row in rows),
    }


def run_probe() -> dict[str, Any]:
    command = [
            "cargo",
            "run",
            "--release",
            "--quiet",
            "--example",
            "runtime_budget_probe",
            "--locked",
        ]
    if platform.system() == "Darwin" and platform.machine() == "arm64":
        command.insert(1, "+stable-aarch64-apple-darwin")
    output = subprocess.run(
        command,
        cwd=ROOT, text=True, capture_output=True, check=True, timeout=180,
    )
    return json.loads(output.stdout)


def evaluate(report: dict[str, Any]) -> dict[str, bool]:
    elapsed = report["cold_start"]["elapsed_ms"]
    rss = report["cold_start"]["process_peak_rss_bytes"]
    probe = report["runtime_probe"]
    gates = {
        "normal_dependencies": report["dependencies"]["normal_count"] <= BUDGETS["normal_dependencies_max"],
        "release_binary": report["release_binary_bytes"] <= BUDGETS["release_binary_bytes_max"],
        "cold_start": elapsed["max"] <= BUDGETS["cold_start_max_ms_max"],
        "cold_start_peak_rss": rss["max"] <= BUDGETS["cold_start_peak_rss_max_bytes"],
        "queue_round_trip": probe["per_operation_us"]["queue_round_trip"] <= BUDGETS["queue_round_trip_max_us"],
        "render": probe["per_operation_us"]["render"] <= BUDGETS["render_max_us"],
        "layout": probe["per_operation_us"]["layout"] <= BUDGETS["layout_max_us"],
        "render_coalescing": probe["scheduler_frames_after_10000_dirty_requests"] <= BUDGETS["coalesced_frames_max"],
    }
    footprint = report.get("headless_footprint")
    if footprint is not None:
        gates["headless_footprint_processes"] = footprint["processes_within_cap"]
        for index, row in enumerate(footprint["rows"]):
            for name, value in row["gates"].items():
                gates[f"headless_footprint[{index}].{name}"] = value
    return gates


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--samples", type=int, default=5, help="cold-start samples (3-50)")
    parser.add_argument("--output", type=pathlib.Path, help="also write the JSON receipt atomically")
    parser.add_argument("--no-fail", action="store_true", help="report budget failures without a nonzero exit")
    parser.add_argument("--near-cap", action="store_true", help="also measure one valid journal near the 256 MiB startup cap")
    parser.add_argument("--headless-footprint", action="store_true",
                        help="also measure per-process idle CPU/RSS for long-lived headless workers")
    parser.add_argument("--headless-workers", type=int, default=1,
                        help="headless workers to sample with --headless-footprint (1-64)")
    parser.add_argument("--startup-evidence-dir", type=pathlib.Path,
                        help="new directory for durable per-sample output/PID/phase receipts; defaults to .ops/runtime-budget/<UTC timestamp>")
    args = parser.parse_args()
    if not 3 <= args.samples <= 50:
        parser.error("--samples must be between 3 and 50")
    if not 1 <= args.headless_workers <= BUDGETS["headless_footprint_processes_max"]:
        parser.error(f"--headless-workers must be between 1 and {BUDGETS['headless_footprint_processes_max']}")
    evidence_dir = args.startup_evidence_dir or ROOT / '.ops/runtime-budget' / dt.datetime.now(dt.timezone.utc).strftime('%Y%m%dT%H%M%S.%fZ')
    if not evidence_dir.is_absolute():
        evidence_dir = ROOT / evidence_dir
    if evidence_dir.exists():
        parser.error('--startup-evidence-dir must be a new directory to preserve earlier samples')
    evidence_dir.mkdir(parents=True)
    dependencies = dependency_receipt()
    binary, cold_start = build_and_measure_startup(args.samples, evidence_dir)
    report: dict[str, Any] = {
        "schema_version": 1,
        "measurement_kind": "reproducible_regression_gate_not_p95",
        "host": {"system": platform.system(), "release": platform.release(), "machine": platform.machine(), "python": platform.python_version()},
        "budgets": BUDGETS,
        "dependencies": dependencies,
        "release_binary_bytes": binary.stat().st_size,
        "cold_start": cold_start,
        "startup_evidence_dir": str(evidence_dir),
        "runtime_probe": run_probe(),
    }
    if args.near_cap:
        report["near_cap_journal"] = build_and_measure_near_cap(binary)
    if args.headless_footprint:
        report["headless_footprint"] = measure_headless_footprint(
            binary, workers=args.headless_workers, samples=args.samples,
        )
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
