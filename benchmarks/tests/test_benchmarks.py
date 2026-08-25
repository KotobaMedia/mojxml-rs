import json
import tempfile
import unittest
from pathlib import Path

from benchmarks import compare
from benchmarks import run as benchmark_run


class RunTests(unittest.TestCase):
    def test_statistics_summary_reports_sample_variability(self):
        summary = benchmark_run.statistics_summary([10.0, 11.0, 12.0, 13.0, 14.0])

        self.assertEqual(summary["sample_count"], 5)
        self.assertEqual(summary["median"], 12.0)
        self.assertEqual(summary["mean"], 12.0)
        self.assertAlmostEqual(summary["standard_deviation"], 1.5811388300841898)
        self.assertLess(summary["mean_ci95_lower"], summary["mean"])
        self.assertGreater(summary["mean_ci95_upper"], summary["mean"])

    def test_dataset_manifest_is_stable_and_path_sensitive(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            first = root / "a.zip"
            second = root / "b.zip"
            first.write_bytes(b"first")
            second.write_bytes(b"second")

            manifest = benchmark_run.create_dataset_manifest(root, [first, second])
            repeated = benchmark_run.create_dataset_manifest(root, [first, second])
            reversed_manifest = benchmark_run.create_dataset_manifest(root, [second, first])

            self.assertEqual(manifest, repeated)
            self.assertNotEqual(manifest["digest"], reversed_manifest["digest"])
            self.assertEqual(manifest["compressed_input_bytes"], 11)

    def test_flatgeobuf_header_validation(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "output.fgb"
            header = b"12345678"
            path.write_bytes(
                benchmark_run.FLATGEOBUF_MAGIC
                + len(header).to_bytes(4, byteorder="little")
                + header
            )
            self.assertEqual(benchmark_run.validate_flatgeobuf(path), 20)

            path.write_bytes(b"not flatgeobuf")
            with self.assertRaises(benchmark_run.BenchmarkError):
                benchmark_run.validate_flatgeobuf(path)

            path.write_bytes(
                benchmark_run.FLATGEOBUF_MAGIC
                + len(header).to_bytes(4, byteorder="little")
                + header[:-1]
            )
            with self.assertRaises(benchmark_run.BenchmarkError):
                benchmark_run.validate_flatgeobuf(path)

    def test_parquet_magic_validation(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "output.parquet"
            path.write_bytes(b"PAR1payloadPAR1")
            self.assertEqual(benchmark_run.validate_parquet(path), 15)

            path.write_bytes(b"not parquet")
            with self.assertRaises(benchmark_run.BenchmarkError):
                benchmark_run.validate_parquet(path)

    def test_geojson_sequence_validation(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "output.geojson"
            feature = b'{"type":"Feature","geometry":{},"properties":{}}'
            path.write_bytes(feature + b"\n" + feature + b"\n")
            self.assertEqual(benchmark_run.validate_geojson(path), 2 * (len(feature) + 1))

            path.write_bytes(feature)
            with self.assertRaises(benchmark_run.BenchmarkError):
                benchmark_run.validate_geojson(path)

    def test_output_format_defaults_to_fgb_and_accepts_geoparquet(self):
        default_args = benchmark_run.parse_args(
            ["--input-dir", "input", "--work-dir", "work"]
        )
        geoparquet_args = benchmark_run.parse_args(
            [
                "--input-dir",
                "input",
                "--work-dir",
                "work",
                "--output-format",
                "geoparquet",
            ]
        )

        self.assertEqual(default_args.output_format, "fgb")
        self.assertEqual(geoparquet_args.output_format, "geoparquet")

    def test_cli_metrics_validation_rejects_parse_errors(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "metrics.json"
            path.write_text(
                json.dumps(
                    {
                        "schema_version": 1,
                        "input_file_count": 1,
                        "input_read_errors": 0,
                        "input_files_without_xml": 0,
                        "xml_documents_discovered": 1,
                        "xml_documents_parsed_ok": 0,
                        "xml_document_parse_errors": 1,
                        "write_error_batches": 0,
                        "written_features": 1,
                        "output_created": True,
                        "output_bytes": 100,
                    }
                )
            )
            with self.assertRaises(benchmark_run.BenchmarkError):
                benchmark_run.load_and_validate_cli_metrics(path, 1, 100)


class CompareTests(unittest.TestCase):
    def report(self, label, median):
        signature = {
            "input_file_count": 1,
            "input_xml_bytes": 10,
            "input_read_errors": 0,
            "input_files_without_xml": 0,
            "xml_documents_discovered": 1,
            "xml_documents_parsed_ok": 1,
            "xml_document_parse_errors": 0,
            "written_batches": 1,
            "written_features": 2,
            "write_error_batches": 0,
            "output_created": True,
            "logical_cpu_count": 8,
            "zip_workers": 1,
            "parse_workers": 6,
        }
        return {
            "schema_version": 1,
            "host": {
                "label": label,
                "os": "Linux",
                "os_release": "test",
                "architecture": "x86_64",
                "cpu_model": "Test CPU",
                "logical_cpu_count": 8,
                "is_wsl": True,
            },
            "source": {"git_commit": "abc", "git_dirty": False, "rustc": "rustc 1.0\nhost"},
            "binary": {"version": "mojxml-rs 1.0"},
            "dataset": {"digest": "dataset", "file_count": 1},
            "configuration": {
                "output_format": "FlatGeobuf",
                "cli_args": [],
                "cache_policy": "warm",
                "warmup_runs": 1,
                "measured_runs": 5,
                "max_cv_percent": 3.0,
            },
            "consistency_signature": signature,
            "summary": {
                "wall_time_seconds": {
                    "median": median,
                    "mean_ci95_lower": median - 1,
                    "mean_ci95_upper": median + 1,
                    "coefficient_of_variation_percent": 1.0,
                },
                "peak_rss_bytes": {"median": 1024**3},
                "quality": {"stable": True},
            },
        }

    def test_comparison_checks_compatibility_and_renders_markdown(self):
        reports = [self.report("baseline", 10.0), self.report("candidate", 5.0)]
        warnings = compare.validate_compatibility(
            reports, [Path("a.json"), Path("b.json")], allow_code_mismatch=False
        )
        table = compare.render_table(reports)

        self.assertEqual(warnings, [])
        self.assertIn("| candidate |", table)
        self.assertIn("2.00×", table)

    def test_comparison_rejects_a_different_dataset(self):
        reports = [self.report("a", 10.0), self.report("b", 10.0)]
        reports[1]["dataset"]["digest"] = "other"
        with self.assertRaises(compare.ComparisonError):
            compare.validate_compatibility(
                reports, [Path("a.json"), Path("b.json")], allow_code_mismatch=False
            )


if __name__ == "__main__":
    unittest.main()
