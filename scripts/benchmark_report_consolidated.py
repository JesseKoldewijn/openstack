#!/usr/bin/env python3

import argparse
import glob
import json
import os
import tempfile
import unittest
from pathlib import Path
from typing import Any, Dict, Optional


LANES = [
    ("fair-low", "Fair Low"),
    ("fair-medium", "Fair Medium"),
    ("fair-high", "Fair High"),
    ("fair-extreme", "Fair Extreme"),
]


def load_latest_report(directory: str, lane: str) -> Optional[Dict[str, Any]]:
    pattern = os.path.join(directory, f"{lane}-*.json")
    matches = sorted(glob.glob(pattern), key=os.path.getmtime)
    if not matches:
        return None
    with open(matches[-1], "r", encoding="utf-8") as handle:
        return json.load(handle)


def fmt(value: Optional[float]) -> str:
    if value is None:
        return "n/a"
    return f"{value:.3f}"


def load_gate_result(directory: str, lane: str) -> Optional[Dict[str, Any]]:
    pattern = os.path.join(directory, f"benchmark-gate-{lane}-*.json")
    matches = sorted(glob.glob(pattern), key=os.path.getmtime)
    if not matches:
        return None
    with open(matches[-1], "r", encoding="utf-8") as handle:
        return json.load(handle)


def build_markdown(report_dir: str, include_gate: bool) -> str:
    lines = ["## Benchmark Consolidated Summary", ""]
    gate_results: Dict[str, Dict[str, Any]] = {}
    if include_gate:
        for lane, _label in LANES:
            gate = load_gate_result(report_dir, lane)
            if gate:
                gate_results[lane] = gate

    if gate_results:
        lines.extend(
            [
                "### Regression Gate Verdicts",
                "",
                "| Lane | Status | p95 threshold | p99 threshold | throughput threshold |",
                "|---|---|---:|---:|---:|",
            ]
        )
        for lane, label in LANES:
            gate = gate_results.get(lane)
            if not gate:
                continue
            t = gate.get("thresholds", {})
            lines.append(
                "| {label} | {status} | +{p95:.1f}% | +{p99:.1f}% | -{tp:.1f}% |".format(
                    label=label,
                    status=gate.get("status", "unknown"),
                    p95=float(t.get("p95_regression_limit_pct", 0.0)),
                    p99=float(t.get("p99_regression_limit_pct", 0.0)),
                    tp=float(t.get("throughput_regression_limit_pct", 0.0)),
                )
            )
        lines.append("")

    lines.extend(
        [
            "| Lane | Scenarios | Performance | Skipped | Avg p95 ratio | Avg p99 ratio | Avg throughput ratio |",
            "|---|---:|---:|---:|---:|---:|---:|",
        ]
    )

    lane_reports: list[tuple[str, Dict[str, Any]]] = []
    for lane, label in LANES:
        report = load_latest_report(report_dir, lane)
        if report is None:
            lines.append(f"| {label} | 0 | 0 | 0 | n/a | n/a | n/a |")
            continue
        summary = report.get("summary", {})
        lines.append(
            "| {label} | {total} | {perf} | {skipped} | {p95} | {p99} | {tp} |".format(
                label=label,
                total=summary.get("total_scenarios", 0),
                perf=summary.get("performance_scenarios", 0),
                skipped=summary.get("skipped_scenarios", 0),
                p95=fmt(summary.get("avg_latency_p95_ratio")),
                p99=fmt(summary.get("avg_latency_p99_ratio")),
                tp=fmt(summary.get("avg_throughput_ratio")),
            )
        )
        lane_reports.append((label, report))

    for label, report in lane_reports:
        per_service = report.get("summary", {}).get("per_service", {})
        lines.extend(["", f"### {label} Per-Service"])
        lines.extend(
            [
                "",
                "| Service | Scenarios | Skipped | p95 ratio | p99 ratio | Throughput ratio |",
                "|---|---:|---:|---:|---:|---:|",
            ]
        )
        for service in sorted(per_service.keys()):
            entry = per_service[service]
            lines.append(
                "| {service} | {scenarios} | {skipped} | {p95} | {p99} | {tp} |".format(
                    service=service,
                    scenarios=entry.get("total_scenarios", 0),
                    skipped=entry.get("skipped_scenarios", 0),
                    p95=fmt(entry.get("avg_latency_p95_ratio")),
                    p99=fmt(entry.get("avg_latency_p99_ratio")),
                    tp=fmt(entry.get("avg_throughput_ratio")),
                )
            )

    return "\n".join(lines).strip() + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description="Build consolidated benchmark summary")
    parser.add_argument("--report-dir", default="target/benchmark-reports")
    parser.add_argument("--summary-path")
    parser.add_argument("--output-path")
    parser.add_argument("--include-gate", action="store_true")
    parser.add_argument("--run-tests", action="store_true")
    args = parser.parse_args()

    if args.run_tests:
        return run_tests()

    content = build_markdown(args.report_dir, args.include_gate)

    if args.summary_path:
        with open(args.summary_path, "a", encoding="utf-8") as handle:
            handle.write(content + "\n")

    if args.output_path:
        Path(args.output_path).write_text(content, encoding="utf-8")
    else:
        print(content)

    return 0


class ConsolidatedReportTests(unittest.TestCase):
    def test_build_markdown_with_lane_and_gate_data(self) -> None:
        with tempfile.TemporaryDirectory(prefix="bench-consolidated-") as temp_dir:
            report = {
                "summary": {
                    "total_scenarios": 2,
                    "performance_scenarios": 2,
                    "skipped_scenarios": 0,
                    "avg_latency_p95_ratio": 1.0,
                    "avg_latency_p99_ratio": 1.1,
                    "avg_throughput_ratio": 0.95,
                    "per_service": {
                        "s3": {
                            "total_scenarios": 1,
                            "skipped_scenarios": 0,
                            "avg_latency_p95_ratio": 1.2,
                            "avg_latency_p99_ratio": 1.3,
                            "avg_throughput_ratio": 0.9,
                        }
                    },
                }
            }
            gate = {
                "status": "pass",
                "thresholds": {
                    "p95_regression_limit_pct": 8.0,
                    "p99_regression_limit_pct": 12.0,
                    "throughput_regression_limit_pct": 8.0,
                },
            }
            Path(temp_dir, "fair-low-123.json").write_text(json.dumps(report), encoding="utf-8")
            Path(temp_dir, "benchmark-gate-fair-low-123.json").write_text(
                json.dumps(gate), encoding="utf-8"
            )

            content = build_markdown(temp_dir, include_gate=True)
            self.assertIn("Benchmark Consolidated Summary", content)
            self.assertIn("Regression Gate Verdicts", content)
            self.assertIn("Fair Low", content)
            self.assertIn("s3", content)


def run_tests() -> int:
    suite = unittest.defaultTestLoader.loadTestsFromTestCase(ConsolidatedReportTests)
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    return 0 if result.wasSuccessful() else 1


if __name__ == "__main__":
    raise SystemExit(main())
