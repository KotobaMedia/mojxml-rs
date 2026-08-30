#!/usr/bin/env python3
"""Compare end-to-end benchmark JSON reports as a Markdown table."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any, Sequence


OUTPUT_SIGNATURE_FIELDS = (
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
)


class ComparisonError(RuntimeError):
    pass


def load_report(path: Path) -> dict[str, Any]:
    try:
        report = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ComparisonError(f"cannot read {path}: {error}") from error
    if report.get("schema_version") != 1 or not report.get("summary"):
        raise ComparisonError(f"{path} is not a completed benchmark schema 1 report")
    return report


def validate_compatibility(
    reports: Sequence[dict[str, Any]], paths: Sequence[Path], allow_code_mismatch: bool
) -> list[str]:
    reference = reports[0]
    warnings: list[str] = []
    for report, path in zip(reports[1:], paths[1:]):
        if report["dataset"]["digest"] != reference["dataset"]["digest"]:
            raise ComparisonError(f"dataset fingerprint differs in {path}")
        for field in (
            "output_format",
            "cli_args",
            "cache_policy",
            "warmup_runs",
            "measured_runs",
            "max_cv_percent",
        ):
            if report["configuration"].get(field) != reference["configuration"].get(field):
                raise ComparisonError(f"benchmark configuration {field!r} differs in {path}")
        for field in OUTPUT_SIGNATURE_FIELDS:
            if report["consistency_signature"].get(field) != reference[
                "consistency_signature"
            ].get(field):
                raise ComparisonError(f"output signature field {field!r} differs in {path}")

        reference_commit = reference["source"].get("git_commit")
        report_commit = report["source"].get("git_commit")
        if report_commit != reference_commit:
            message = f"source commit differs in {path}: {report_commit} != {reference_commit}"
            if not allow_code_mismatch:
                raise ComparisonError(message)
            warnings.append(message)
        if report["binary"].get("version") != reference["binary"].get("version"):
            message = f"binary version differs in {path}"
            if not allow_code_mismatch:
                raise ComparisonError(message)
            warnings.append(message)
        reference_rustc = (reference["source"].get("rustc") or "").splitlines()[:1]
        report_rustc = (report["source"].get("rustc") or "").splitlines()[:1]
        if report_rustc != reference_rustc:
            message = f"Rust compiler version differs in {path}"
            if not allow_code_mismatch:
                raise ComparisonError(message)
            warnings.append(message)

    dirty_paths = [str(path) for report, path in zip(reports, paths) if report["source"]["git_dirty"]]
    if dirty_paths:
        message = "uncommitted source changes were recorded by: " + ", ".join(dirty_paths)
        if not allow_code_mismatch:
            raise ComparisonError(message + " (pass --allow-code-mismatch to compare anyway)")
        warnings.append(message)
    unstable_paths = [
        str(path)
        for report, path in zip(reports, paths)
        if report["summary"].get("quality", {}).get("stable") is False
    ]
    if unstable_paths:
        raise ComparisonError(
            "wall-time CV failed the quality threshold in: " + ", ".join(unstable_paths)
        )
    return warnings


def markdown_escape(value: Any) -> str:
    return str(value).replace("|", "\\|").replace("\n", " ")


def gibibytes(byte_count: float) -> str:
    return f"{byte_count / (1024 ** 3):.2f} GiB"


def duration(seconds: float) -> str:
    if seconds >= 120:
        return f"{seconds / 60:.2f} min"
    return f"{seconds:.3f} s"


def render_table(reports: Sequence[dict[str, Any]]) -> str:
    baseline_median = reports[0]["summary"]["wall_time_seconds"]["median"]
    lines = [
        "| Environment | OS / architecture | CPU | Logical CPUs | Workers (read/parse) | "
        "Median wall time | Mean 95% CI | CV | Median peak RSS | Relative throughput |",
        "|---|---|---|---:|---:|---:|---:|---:|---:|---:|",
    ]
    for report in reports:
        host = report["host"]
        wall = report["summary"]["wall_time_seconds"]
        peak = report["summary"]["peak_rss_bytes"]
        signature = report["consistency_signature"]
        relative = baseline_median / wall["median"]
        os_name = f"{host['os']} {host['os_release']} / {host['architecture']}"
        if host.get("is_wsl"):
            os_name = "WSL2 " + os_name
        ci = f"{duration(wall['mean_ci95_lower'])}–{duration(wall['mean_ci95_upper'])}"
        cells = [
            host["label"],
            os_name,
            host.get("cpu_model") or "unknown",
            signature.get("logical_cpu_count") or "unknown",
            f"{signature['zip_workers']}/{signature['parse_workers']}",
            duration(wall["median"]),
            ci,
            f"{wall['coefficient_of_variation_percent']:.2f}%",
            gibibytes(peak["median"]),
            f"{relative:.2f}×",
        ]
        lines.append("| " + " | ".join(markdown_escape(cell) for cell in cells) + " |")
    return "\n".join(lines)


def parse_args(argv: Sequence[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Compare mojxml-rs benchmark reports.")
    parser.add_argument("reports", nargs="+", type=Path)
    parser.add_argument(
        "--allow-code-mismatch",
        action="store_true",
        help="Allow different commits, versions, or dirty trees (dataset/config must still match).",
    )
    args = parser.parse_args(argv)
    if len(args.reports) < 2:
        parser.error("provide at least two result.json files")
    return args


def main(argv: Sequence[str] | None = None) -> int:
    args = parse_args(argv if argv is not None else sys.argv[1:])
    reports = [load_report(path) for path in args.reports]
    warnings = validate_compatibility(reports, args.reports, args.allow_code_mismatch)
    print(render_table(reports))
    dataset = reports[0]["dataset"]
    signature = reports[0]["consistency_signature"]
    print()
    print(
        f"Dataset `{dataset['digest']}` ({dataset['file_count']} compressed inputs); "
        f"{signature['xml_documents_discovered']} XML documents; "
        f"{signature['written_features']} output features."
    )
    for warning in warnings:
        print(f"warning: {warning}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except ComparisonError as error:
        print(f"comparison error: {error}", file=sys.stderr)
        raise SystemExit(1)
