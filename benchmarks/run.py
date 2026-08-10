#!/usr/bin/env python3
"""Run reproducible end-to-end mojxml-rs GeoParquet benchmarks.

The runner intentionally uses only the Python standard library.  It supports the
two environments used for the project benchmark: Linux (including WSL2) and
macOS.
"""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import math
import os
import platform
import re
import shutil
import statistics
import subprocess
import sys
import time
from pathlib import Path
from typing import Any, Iterable, Sequence


SCHEMA_VERSION = 1
MIB = 1024 * 1024
REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_BINARY = REPO_ROOT / "target" / "release" / "mojxml-rs"
SIGNATURE_FIELDS = (
    "input_file_count",
    "input_xml_bytes",
    "input_read_errors",
    "input_files_without_xml",
    "xml_documents_discovered",
    "xml_documents_parsed_ok",
    "xml_document_parse_errors",
    "written_batches",
    "written_features",
    "write_error_batches",
    "output_created",
    "logical_cpu_count",
    "zip_workers",
    "parse_workers",
)


class BenchmarkError(RuntimeError):
    pass


def utc_now() -> str:
    return dt.datetime.now(dt.timezone.utc).isoformat(timespec="seconds")


def command_output(command: Sequence[str], cwd: Path | None = None) -> str | None:
    try:
        result = subprocess.run(
            command,
            cwd=cwd,
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
        )
    except (FileNotFoundError, subprocess.CalledProcessError):
        return None
    value = result.stdout.strip()
    return value or None


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        while chunk := handle.read(8 * MIB):
            digest.update(chunk)
    return digest.hexdigest()


def write_json(path: Path, value: Any) -> None:
    temporary_path = path.with_name(f".{path.name}.tmp")
    with temporary_path.open("w", encoding="utf-8") as handle:
        json.dump(value, handle, ensure_ascii=False, indent=2, sort_keys=True)
        handle.write("\n")
    os.replace(temporary_path, path)


def discover_inputs(input_dir: Path, patterns: Sequence[str]) -> list[Path]:
    if not input_dir.is_dir():
        raise BenchmarkError(f"input directory does not exist: {input_dir}")

    by_relative_path: dict[str, Path] = {}
    for pattern in patterns:
        for path in input_dir.glob(pattern):
            if not path.is_file():
                continue
            relative_path = path.relative_to(input_dir).as_posix()
            if path.suffix.lower() not in {".zip", ".xml"}:
                raise BenchmarkError(
                    f"input pattern selected an unsupported file: {relative_path}"
                )
            by_relative_path[relative_path] = path.absolute()

    if not by_relative_path:
        rendered_patterns = ", ".join(patterns)
        raise BenchmarkError(
            f"no .zip or .xml files matched {rendered_patterns!r} in {input_dir}"
        )
    return [by_relative_path[key] for key in sorted(by_relative_path)]


def create_dataset_manifest(input_dir: Path, inputs: Sequence[Path]) -> dict[str, Any]:
    entries: list[dict[str, Any]] = []
    total_bytes = 0
    print(f"Fingerprinting {len(inputs)} input file(s) with SHA-256...", flush=True)
    for index, path in enumerate(inputs, start=1):
        size = path.stat().st_size
        entry = {
            "path": path.relative_to(input_dir).as_posix(),
            "bytes": size,
            "sha256": sha256_file(path),
        }
        entries.append(entry)
        total_bytes += size
        if len(inputs) >= 100 and (index % 100 == 0 or index == len(inputs)):
            print(f"  fingerprinted {index}/{len(inputs)}", flush=True)

    canonical_entries = json.dumps(
        entries, ensure_ascii=False, sort_keys=True, separators=(",", ":")
    ).encode("utf-8")
    return {
        "schema_version": 1,
        "algorithm": "sha256(path, size, and per-file sha256)",
        "digest": hashlib.sha256(canonical_entries).hexdigest(),
        "file_count": len(entries),
        "compressed_input_bytes": total_bytes,
        "files": entries,
    }


def read_first_matching_line(path: Path, prefix: str) -> str | None:
    try:
        with path.open(encoding="utf-8", errors="replace") as handle:
            for line in handle:
                if line.startswith(prefix):
                    return line.split(":", 1)[-1].strip()
    except OSError:
        return None
    return None


def total_memory_bytes() -> int | None:
    if sys.platform.startswith("linux"):
        memory_kib = read_first_matching_line(Path("/proc/meminfo"), "MemTotal:")
        if memory_kib:
            return int(memory_kib.split()[0]) * 1024
    if sys.platform == "darwin":
        value = command_output(["sysctl", "-n", "hw.memsize"])
        if value and value.isdigit():
            return int(value)
    try:
        return os.sysconf("SC_PHYS_PAGES") * os.sysconf("SC_PAGE_SIZE")
    except (ValueError, OSError, AttributeError):
        return None


def cpu_model() -> str | None:
    if sys.platform.startswith("linux"):
        return read_first_matching_line(Path("/proc/cpuinfo"), "model name")
    if sys.platform == "darwin":
        return command_output(["sysctl", "-n", "machdep.cpu.brand_string"]) or command_output(
            ["sysctl", "-n", "hw.model"]
        )
    return platform.processor() or None


def physical_cpu_count() -> int | None:
    if sys.platform == "darwin":
        value = command_output(["sysctl", "-n", "hw.physicalcpu"])
        if value and value.isdigit():
            return int(value)
    if sys.platform.startswith("linux"):
        try:
            pairs: set[tuple[str, str]] = set()
            physical_id = "0"
            core_id: str | None = None
            for line in Path("/proc/cpuinfo").read_text(errors="replace").splitlines():
                if not line.strip():
                    if core_id is not None:
                        pairs.add((physical_id, core_id))
                    physical_id, core_id = "0", None
                    continue
                key, _, value = line.partition(":")
                if key.strip() == "physical id":
                    physical_id = value.strip()
                elif key.strip() == "core id":
                    core_id = value.strip()
            if core_id is not None:
                pairs.add((physical_id, core_id))
            if pairs:
                return len(pairs)
        except OSError:
            pass
    return None


def filesystem_description(path: Path) -> str | None:
    if sys.platform.startswith("linux"):
        return command_output(["findmnt", "-no", "SOURCE,FSTYPE,TARGET", "--target", str(path)])
    return command_output(["df", "-P", str(path)])


def collect_host_metadata(label: str, input_dir: Path, work_dir: Path) -> dict[str, Any]:
    kernel_version = platform.release()
    proc_version = ""
    if sys.platform.startswith("linux"):
        try:
            proc_version = Path("/proc/version").read_text(errors="replace").strip()
        except OSError:
            pass
    is_wsl = "microsoft" in f"{kernel_version} {proc_version}".lower()
    available_cpu_count = os.cpu_count()
    if hasattr(os, "sched_getaffinity"):
        available_cpu_count = len(os.sched_getaffinity(0))
    return {
        "label": label,
        "os": platform.system(),
        "os_release": platform.release(),
        "os_version": platform.version(),
        "architecture": platform.machine(),
        "cpu_model": cpu_model(),
        "logical_cpu_count": available_cpu_count,
        "hardware_logical_cpu_count": os.cpu_count(),
        "physical_cpu_count": physical_cpu_count(),
        "memory_bytes": total_memory_bytes(),
        "is_wsl": is_wsl,
        "python_version": platform.python_version(),
        "input_filesystem": filesystem_description(input_dir),
        "work_filesystem": filesystem_description(work_dir),
    }


def collect_source_metadata() -> dict[str, Any]:
    commit = command_output(["git", "rev-parse", "HEAD"], cwd=REPO_ROOT)
    status = command_output(["git", "status", "--short"], cwd=REPO_ROOT)
    return {
        "git_commit": commit,
        "git_dirty": bool(status),
        "git_status": status.splitlines() if status else [],
        "rustc": command_output(["rustc", "-Vv"], cwd=REPO_ROOT),
        "cargo": command_output(["cargo", "-V"], cwd=REPO_ROOT),
        "rustflags": os.environ.get("RUSTFLAGS"),
        "cargo_build_target": os.environ.get("CARGO_BUILD_TARGET"),
    }


def build_binary(binary: Path) -> None:
    if binary != DEFAULT_BINARY:
        raise BenchmarkError(
            "--binary points outside target/release; pass --no-build to benchmark that binary"
        )
    print("Building the benchmark binary (release, locked dependencies)...", flush=True)
    subprocess.run(
        ["cargo", "build", "--release", "--locked", "-p", "mojxml-rs"],
        cwd=REPO_ROOT,
        check=True,
    )


def max_rss_bytes(raw_max_rss: int) -> int:
    # getrusage(2) reports bytes on macOS and KiB on Linux.
    return raw_max_rss if sys.platform == "darwin" else raw_max_rss * 1024


def run_child(
    command: Sequence[str], stdout_path: Path, stderr_path: Path
) -> tuple[int, float, dict[str, Any]]:
    if not hasattr(os, "fork") or not hasattr(os, "wait4"):
        raise BenchmarkError("this benchmark runner requires POSIX os.fork() and os.wait4()")

    environment = os.environ.copy()
    environment.update({"LC_ALL": "C", "LANG": "C", "TZ": "UTC"})
    started = time.perf_counter()
    pid = os.fork()
    if pid == 0:
        try:
            stdout_fd = os.open(stdout_path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o644)
            stderr_fd = os.open(stderr_path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o644)
            os.dup2(stdout_fd, 1)
            os.dup2(stderr_fd, 2)
            os.close(stdout_fd)
            os.close(stderr_fd)
            os.chdir(REPO_ROOT)
            os.execve(command[0], list(command), environment)
        except BaseException as error:
            message = f"benchmark exec failed: {error}\n".encode("utf-8", errors="replace")
            try:
                os.write(2, message)
            finally:
                os._exit(127)

    while True:
        try:
            _, status, usage = os.wait4(pid, 0)
            break
        except InterruptedError:
            continue
    wall_time = time.perf_counter() - started
    exit_code = os.waitstatus_to_exitcode(status)
    resource_usage = {
        "user_cpu_seconds": usage.ru_utime,
        "system_cpu_seconds": usage.ru_stime,
        "peak_rss_bytes": max_rss_bytes(usage.ru_maxrss),
        "major_page_faults": usage.ru_majflt,
        "minor_page_faults": usage.ru_minflt,
        "input_blocks": usage.ru_inblock,
        "output_blocks": usage.ru_oublock,
        "voluntary_context_switches": usage.ru_nvcsw,
        "involuntary_context_switches": usage.ru_nivcsw,
    }
    return exit_code, wall_time, resource_usage


def validate_parquet(path: Path) -> int:
    try:
        size = path.stat().st_size
        with path.open("rb") as handle:
            header = handle.read(4)
            handle.seek(-4, os.SEEK_END)
            footer = handle.read(4)
    except (OSError, ValueError) as error:
        raise BenchmarkError(f"cannot inspect benchmark output {path}: {error}") from error
    if size < 12 or header != b"PAR1" or footer != b"PAR1":
        raise BenchmarkError(f"output is not a complete Parquet file: {path}")
    return size


def load_and_validate_cli_metrics(path: Path, input_count: int, output_bytes: int) -> dict[str, Any]:
    try:
        metrics = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise BenchmarkError(f"cannot read CLI metrics {path}: {error}") from error

    problems: list[str] = []
    if metrics.get("schema_version") != 1:
        problems.append(f"unsupported CLI metrics schema {metrics.get('schema_version')!r}")
    if metrics.get("input_file_count") != input_count:
        problems.append(
            f"CLI saw {metrics.get('input_file_count')} inputs; expected {input_count}"
        )
    if metrics.get("xml_documents_discovered", 0) <= 0:
        problems.append("no XML documents were discovered")
    if metrics.get("input_read_errors") != 0:
        problems.append(f"{metrics.get('input_read_errors')} input read error(s)")
    if metrics.get("input_files_without_xml") != 0:
        problems.append(
            f"{metrics.get('input_files_without_xml')} input file(s) contained no XML"
        )
    if metrics.get("xml_documents_parsed_ok") != metrics.get("xml_documents_discovered"):
        problems.append("not every discovered XML document parsed successfully")
    if metrics.get("xml_document_parse_errors") != 0:
        problems.append(f"{metrics.get('xml_document_parse_errors')} XML parse error(s)")
    if metrics.get("write_error_batches") != 0:
        problems.append(f"{metrics.get('write_error_batches')} writer error(s)")
    if metrics.get("written_features", 0) <= 0:
        problems.append("no features were written")
    if not metrics.get("output_created"):
        problems.append("the CLI did not report an output file")
    if metrics.get("output_bytes") != output_bytes:
        problems.append(
            f"CLI reported {metrics.get('output_bytes')} output bytes; observed {output_bytes}"
        )
    if problems:
        raise BenchmarkError("invalid benchmark run: " + "; ".join(problems))
    return metrics


def tail(path: Path, line_count: int = 30) -> str:
    try:
        lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
    except OSError:
        return ""
    return "\n".join(lines[-line_count:])


def run_once(
    *,
    kind: str,
    index: int,
    binary: Path,
    inputs: Sequence[Path],
    run_dir: Path,
    temp_dir: Path,
    cli_args: Sequence[str],
    keep_outputs: bool,
) -> dict[str, Any]:
    stem = f"{kind}-{index:02d}"
    run_temp_dir = temp_dir / stem
    run_temp_dir.mkdir()
    output_path = run_dir / f"{stem}.parquet"
    metrics_path = run_dir / f"{stem}-cli-metrics.json"
    stdout_path = run_dir / f"{stem}.stdout.log"
    stderr_path = run_dir / f"{stem}.stderr.log"
    command = [
        str(binary),
        "--temp-dir",
        str(run_temp_dir),
        "--metrics-json",
        str(metrics_path),
        *cli_args,
        str(output_path),
        *(str(path) for path in inputs),
    ]

    print(f"Starting {kind} {index}: output is captured in {stdout_path.name}", flush=True)
    exit_code, wall_time, resource_usage = run_child(command, stdout_path, stderr_path)
    if exit_code != 0:
        stderr_tail = tail(stderr_path)
        detail = f"\n{stderr_tail}" if stderr_tail else ""
        raise BenchmarkError(f"{kind} {index} exited with code {exit_code}{detail}")

    output_bytes = validate_parquet(output_path)
    cli_metrics = load_and_validate_cli_metrics(metrics_path, len(inputs), output_bytes)
    total_cpu_seconds = (
        resource_usage["user_cpu_seconds"] + resource_usage["system_cpu_seconds"]
    )
    result = {
        "kind": kind,
        "index": index,
        "external_wall_time_seconds": wall_time,
        "resource_usage": resource_usage,
        "cpu_utilization_percent": total_cpu_seconds / wall_time * 100.0,
        "output_bytes": output_bytes,
        "stdout_log": stdout_path.name,
        "stderr_log": stderr_path.name,
        "cli_metrics": cli_metrics,
    }
    print(
        f"Finished {kind} {index}: {wall_time:.3f} s, "
        f"peak RSS {resource_usage['peak_rss_bytes'] / MIB:.1f} MiB, "
        f"{cli_metrics['written_features']} features",
        flush=True,
    )
    if not keep_outputs:
        output_path.unlink()
    else:
        result["output_file"] = output_path.name
    shutil.rmtree(run_temp_dir)
    return result


def t_critical_95(df: int) -> float:
    values = {
        1: 12.706,
        2: 4.303,
        3: 3.182,
        4: 2.776,
        5: 2.571,
        6: 2.447,
        7: 2.365,
        8: 2.306,
        9: 2.262,
        10: 2.228,
        11: 2.201,
        12: 2.179,
        13: 2.160,
        14: 2.145,
        15: 2.131,
        16: 2.120,
        17: 2.110,
        18: 2.101,
        19: 2.093,
        20: 2.086,
        25: 2.060,
        30: 2.042,
        40: 2.021,
        60: 2.000,
        120: 1.980,
    }
    for lower_df in sorted(values, reverse=True):
        if df >= lower_df:
            return values[lower_df]
    return 1.960


def statistics_summary(values: Iterable[float]) -> dict[str, float | int]:
    samples = list(values)
    if not samples:
        raise BenchmarkError("cannot summarize zero samples")
    mean = statistics.fmean(samples)
    standard_deviation = statistics.stdev(samples) if len(samples) > 1 else 0.0
    ci_half_width = (
        t_critical_95(len(samples) - 1) * standard_deviation / math.sqrt(len(samples))
        if len(samples) > 1
        else 0.0
    )
    return {
        "sample_count": len(samples),
        "median": statistics.median(samples),
        "mean": mean,
        "standard_deviation": standard_deviation,
        "coefficient_of_variation_percent": (
            standard_deviation / mean * 100.0 if mean else 0.0
        ),
        "minimum": min(samples),
        "maximum": max(samples),
        "mean_ci95_lower": mean - ci_half_width,
        "mean_ci95_upper": mean + ci_half_width,
    }


def validate_consistency(runs: Sequence[dict[str, Any]]) -> dict[str, Any]:
    reference = runs[0]["cli_metrics"]
    mismatches: list[str] = []
    for run in runs[1:]:
        metrics = run["cli_metrics"]
        changed = [field for field in SIGNATURE_FIELDS if metrics.get(field) != reference.get(field)]
        if changed:
            mismatches.append(f"{run['kind']} {run['index']}: {', '.join(changed)}")
    if mismatches:
        raise BenchmarkError("run outputs were inconsistent: " + "; ".join(mismatches))
    return {field: reference[field] for field in SIGNATURE_FIELDS}


def summarize_runs(runs: Sequence[dict[str, Any]], max_cv_percent: float) -> dict[str, Any]:
    wall_times = [run["external_wall_time_seconds"] for run in runs]
    peak_rss = [run["resource_usage"]["peak_rss_bytes"] for run in runs]
    cpu_utilization = [run["cpu_utilization_percent"] for run in runs]
    xml_mib = runs[0]["cli_metrics"]["input_xml_bytes"] / MIB
    feature_count = runs[0]["cli_metrics"]["written_features"]
    wall_summary = statistics_summary(wall_times)
    stable = wall_summary["coefficient_of_variation_percent"] <= max_cv_percent
    return {
        "wall_time_seconds": wall_summary,
        "peak_rss_bytes": statistics_summary(peak_rss),
        "cpu_utilization_percent": statistics_summary(cpu_utilization),
        "input_xml_mib_per_second": statistics_summary(
            xml_mib / wall_time for wall_time in wall_times
        ),
        "features_per_second": statistics_summary(
            feature_count / wall_time for wall_time in wall_times
        ),
        "quality": {
            "maximum_cv_percent": max_cv_percent,
            "stable": stable,
            "note": "Warm-up runs are excluded from every summary statistic.",
        },
    }


def safe_label(value: str) -> str:
    cleaned = re.sub(r"[^A-Za-z0-9._-]+", "-", value).strip("-.")
    return cleaned[:60] or "host"


def parse_args(argv: Sequence[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Repeat the complete MOJ XML to GeoParquet conversion with validation."
    )
    parser.add_argument("--input-dir", required=True, type=Path)
    parser.add_argument(
        "--pattern",
        action="append",
        help="Path.glob pattern relative to --input-dir; repeatable (default: *.zip).",
    )
    parser.add_argument(
        "--work-dir",
        required=True,
        type=Path,
        help="Local-disk directory under which a new benchmark result directory is created.",
    )
    parser.add_argument("--binary", type=Path, default=DEFAULT_BINARY)
    parser.add_argument("--no-build", action="store_true")
    parser.add_argument("--warmups", type=int, default=1)
    parser.add_argument("--runs", type=int, default=5)
    parser.add_argument("--max-cv-percent", type=float, default=3.0)
    parser.add_argument(
        "--allow-unstable",
        action="store_true",
        help="Return success even when measured wall-time CV exceeds the threshold.",
    )
    parser.add_argument("--keep-outputs", action="store_true")
    parser.add_argument(
        "--label", default=platform.node(), help="Short environment label stored in the report."
    )
    parser.add_argument(
        "--cli-arg",
        action="append",
        default=[],
        help="Additional mojxml-rs option; use --cli-arg=--arbitrary for leading dashes.",
    )
    args = parser.parse_args(argv)
    if args.warmups < 0:
        parser.error("--warmups must be at least 0")
    if args.runs < 2:
        parser.error("--runs must be at least 2 for a variability estimate")
    if args.max_cv_percent <= 0:
        parser.error("--max-cv-percent must be greater than 0")
    return args


def main(argv: Sequence[str] | None = None) -> int:
    args = parse_args(argv if argv is not None else sys.argv[1:])
    if not (sys.platform.startswith("linux") or sys.platform == "darwin"):
        raise BenchmarkError("supported platforms are Linux/WSL2 and macOS")

    input_dir = args.input_dir.expanduser().resolve()
    work_dir = args.work_dir.expanduser().resolve()
    binary = args.binary.expanduser()
    if not binary.is_absolute():
        binary = (Path.cwd() / binary).resolve()
    patterns = args.pattern or ["*.zip"]
    inputs = discover_inputs(input_dir, patterns)

    work_dir.mkdir(parents=True, exist_ok=True)
    if not args.no_build:
        custom_build_variables = [
            name
            for name in ("RUSTFLAGS", "CARGO_BUILD_TARGET")
            if os.environ.get(name)
        ]
        if custom_build_variables:
            names = ", ".join(custom_build_variables)
            raise BenchmarkError(
                f"unset {names} for the standard benchmark build, or pass --no-build "
                "with an intentionally prebuilt binary"
            )
        build_binary(binary)
    if not binary.is_file() or not os.access(binary, os.X_OK):
        raise BenchmarkError(f"benchmark binary is not executable: {binary}")

    source = collect_source_metadata()
    commit_suffix = (source.get("git_commit") or "unknown")[:8]
    timestamp = dt.datetime.now(dt.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    run_dir = work_dir / (
        f"mojxml-{safe_label(args.label)}-{timestamp}-{commit_suffix}-{os.getpid()}"
    )
    run_dir.mkdir()
    temp_dir = run_dir / "temp"
    temp_dir.mkdir()
    partial_path = run_dir / "result.partial.json"
    result_path = run_dir / "result.json"

    dataset_manifest = create_dataset_manifest(input_dir, inputs)
    write_json(run_dir / "dataset-manifest.json", dataset_manifest)
    binary_sha256 = sha256_file(binary)
    report: dict[str, Any] = {
        "schema_version": SCHEMA_VERSION,
        "benchmark": "mojxml-rs end-to-end GeoParquet conversion",
        "started_at_utc": utc_now(),
        "finished_at_utc": None,
        "host": collect_host_metadata(args.label, input_dir, work_dir),
        "source": source,
        "binary": {
            "path": str(binary),
            "sha256": binary_sha256,
            "version": command_output([str(binary), "--version"]),
        },
        "dataset": {
            "manifest": "dataset-manifest.json",
            "digest": dataset_manifest["digest"],
            "file_count": dataset_manifest["file_count"],
            "compressed_input_bytes": dataset_manifest["compressed_input_bytes"],
        },
        "configuration": {
            "warmup_runs": args.warmups,
            "measured_runs": args.runs,
            "output_format": "GeoParquet",
            "cli_args": args.cli_arg,
            "input_patterns": patterns,
            "output_retained": args.keep_outputs,
            "max_cv_percent": args.max_cv_percent,
            "cache_policy": "one full warm-up, then consecutive measured runs",
            "locale": "C",
        },
        "runs": [],
        "consistency_signature": None,
        "summary": None,
    }
    write_json(partial_path, report)

    try:
        for index in range(1, args.warmups + 1):
            report["runs"].append(
                run_once(
                    kind="warmup",
                    index=index,
                    binary=binary,
                    inputs=inputs,
                    run_dir=run_dir,
                    temp_dir=temp_dir,
                    cli_args=args.cli_arg,
                    keep_outputs=args.keep_outputs,
                )
            )
            write_json(partial_path, report)

        for index in range(1, args.runs + 1):
            report["runs"].append(
                run_once(
                    kind="measured",
                    index=index,
                    binary=binary,
                    inputs=inputs,
                    run_dir=run_dir,
                    temp_dir=temp_dir,
                    cli_args=args.cli_arg,
                    keep_outputs=args.keep_outputs,
                )
            )
            write_json(partial_path, report)

        report["consistency_signature"] = validate_consistency(report["runs"])
        measured_runs = [run for run in report["runs"] if run["kind"] == "measured"]
        report["summary"] = summarize_runs(measured_runs, args.max_cv_percent)
        report["finished_at_utc"] = utc_now()
        write_json(result_path, report)
        partial_path.unlink()
    except BaseException:
        write_json(partial_path, report)
        raise

    wall = report["summary"]["wall_time_seconds"]
    quality = report["summary"]["quality"]
    print(f"Result: {result_path}")
    print(
        f"Median {wall['median']:.3f} s; mean {wall['mean']:.3f} s; "
        f"CV {wall['coefficient_of_variation_percent']:.2f}%"
    )
    if not quality["stable"]:
        print(
            f"UNSTABLE: wall-time CV exceeds {args.max_cv_percent:.2f}%; "
            "remove background load and repeat the benchmark.",
            file=sys.stderr,
        )
        return 0 if args.allow_unstable else 2
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except BenchmarkError as error:
        print(f"benchmark error: {error}", file=sys.stderr)
        raise SystemExit(1)
