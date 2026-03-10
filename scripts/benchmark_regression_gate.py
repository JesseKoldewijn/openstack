#!/usr/bin/env python3

import argparse
import glob
import json
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path
from typing import Any, Dict, Optional


def run_json(command: list[str]) -> Dict[str, Any]:
    output = subprocess.check_output(command, text=True)
    return json.loads(output)


def fetch_previous_report(
    repo: str,
    workflow_file: str,
    artifact_name: str,
    current_run_id: str,
) -> tuple[Optional[str], Optional[str]]:
    try:
        runs_data = run_json(
            [
                "gh",
                "api",
                f"repos/{repo}/actions/workflows/{workflow_file}/runs?status=success&per_page=30",
            ]
        )
    except Exception:
        return None, None

    for run in runs_data.get("workflow_runs", []):
        run_id = str(run.get("id"))
        if not run_id or run_id == current_run_id:
            continue

        try:
            artifacts_data = run_json(
                [
                    "gh",
                    "api",
                    f"repos/{repo}/actions/runs/{run_id}/artifacts?per_page=100",
                ]
            )
        except Exception:
            continue

        target = None
        for artifact in artifacts_data.get("artifacts", []):
            if artifact.get("expired"):
                continue
            if artifact.get("name") == artifact_name:
                target = artifact
                break
        if not target:
            continue

        temp_dir = tempfile.mkdtemp(prefix="benchmark-gate-baseline-")
        try:
            subprocess.check_call(
                ["gh", "run", "download", run_id, "-n", artifact_name, "-D", temp_dir]
            )
            reports = sorted(glob.glob(f"{temp_dir}/**/*.json", recursive=True), key=os.path.getmtime)
            if reports:
                return reports[-1], run_id
        except Exception:
            pass
        finally:
            shutil.rmtree(temp_dir, ignore_errors=True)

    return None, None


def load_report(path: str) -> Dict[str, Any]:
    with open(path, "r", encoding="utf-8") as handle:
        return json.load(handle)


def lane_metrics(report: Dict[str, Any]) -> Dict[str, Optional[float]]:
    summary = report.get("summary", {})
    return {
        "p95": summary.get("avg_latency_p95_ratio"),
        "p99": summary.get("avg_latency_p99_ratio"),
        "throughput": summary.get("avg_throughput_ratio"),
    }


def evaluate_gate(
    lane: str,
    current_report: Dict[str, Any],
    previous_report: Optional[Dict[str, Any]],
    p95_regression_limit_pct: float,
    p99_regression_limit_pct: float,
    throughput_regression_limit_pct: float,
    strict_missing_baseline: bool,
) -> Dict[str, Any]:
    summary = current_report.get("summary", {})
    perf_scenarios = int(summary.get("performance_scenarios", 0))
    skipped_scenarios = int(summary.get("skipped_scenarios", 0))

    failures: list[str] = []
    checks: list[Dict[str, Any]] = []

    if perf_scenarios == 0:
        failures.append(f"lane {lane}: no performance scenarios in current report")
    if perf_scenarios > 0 and skipped_scenarios >= perf_scenarios:
        failures.append(f"lane {lane}: all performance scenarios are skipped")

    current = lane_metrics(current_report)
    previous = lane_metrics(previous_report) if previous_report else None

    if previous is None:
        if strict_missing_baseline:
            failures.append(
                f"lane {lane}: baseline missing for required lane (seed a successful baseline run first)"
            )
    else:
        latency_checks = [
            ("p95", p95_regression_limit_pct),
            ("p99", p99_regression_limit_pct),
        ]
        for metric, threshold in latency_checks:
            c = current.get(metric)
            p = previous.get(metric)
            if c is None or p is None:
                failures.append(f"lane {lane}: missing metric '{metric}' in current or baseline")
                continue
            limit = float(p) * (1.0 + (threshold / 100.0))
            ok = float(c) <= limit
            checks.append(
                {
                    "metric": metric,
                    "current": c,
                    "baseline": p,
                    "threshold_pct": threshold,
                    "limit": limit,
                    "ok": ok,
                }
            )
            if not ok:
                failures.append(
                    f"lane {lane}: {metric} ratio regressed (current={c:.3f}, baseline={p:.3f}, allowed<={limit:.3f})"
                )

        c = current.get("throughput")
        p = previous.get("throughput")
        if c is None or p is None:
            failures.append(f"lane {lane}: missing metric 'throughput' in current or baseline")
        else:
            limit = float(p) * (1.0 - (throughput_regression_limit_pct / 100.0))
            ok = float(c) >= limit
            checks.append(
                {
                    "metric": "throughput",
                    "current": c,
                    "baseline": p,
                    "threshold_pct": throughput_regression_limit_pct,
                    "limit": limit,
                    "ok": ok,
                }
            )
            if not ok:
                failures.append(
                    f"lane {lane}: throughput ratio regressed (current={c:.3f}, baseline={p:.3f}, allowed>={limit:.3f})"
                )

    return {
        "lane": lane,
        "status": "pass" if not failures else "fail",
        "performance_scenarios": perf_scenarios,
        "skipped_scenarios": skipped_scenarios,
        "current": current,
        "baseline": previous,
        "checks": checks,
        "failures": failures,
        "thresholds": {
            "p95_regression_limit_pct": p95_regression_limit_pct,
            "p99_regression_limit_pct": p99_regression_limit_pct,
            "throughput_regression_limit_pct": throughput_regression_limit_pct,
        },
    }


def format_markdown(result: Dict[str, Any], baseline_run_id: Optional[str]) -> str:
    lane = result["lane"]
    status = result["status"].upper()
    lines = [f"## Benchmark Gate ({lane})", "", f"Status: **{status}**", ""]
    lines.extend(
        [
            "| Metric | Current | Baseline | Threshold | Verdict |",
            "|---|---:|---:|---:|---|",
        ]
    )

    threshold_map = {
        "p95": f"+{result['thresholds']['p95_regression_limit_pct']:.1f}% max",
        "p99": f"+{result['thresholds']['p99_regression_limit_pct']:.1f}% max",
        "throughput": f"-{result['thresholds']['throughput_regression_limit_pct']:.1f}% max",
    }
    for check in result["checks"]:
        metric = check["metric"]
        verdict = "pass" if check["ok"] else "fail"
        lines.append(
            "| {metric} | {current:.3f} | {baseline:.3f} | {threshold} | {verdict} |".format(
                metric=metric,
                current=float(check["current"]),
                baseline=float(check["baseline"]),
                threshold=threshold_map.get(metric, "n/a"),
                verdict=verdict,
            )
        )

    if baseline_run_id:
        lines.extend(["", f"Baseline run id: `{baseline_run_id}`"])

    if result["failures"]:
        lines.extend(["", "Failures:"])
        for failure in result["failures"]:
            lines.append(f"- {failure}")

    return "\n".join(lines).strip() + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description="Benchmark regression gate")
    parser.add_argument("--lane")
    parser.add_argument("--current", help="Path to current benchmark report")
    parser.add_argument("--current-glob", help="Glob for current benchmark report")
    parser.add_argument("--previous", help="Path to baseline report")
    parser.add_argument("--repo")
    parser.add_argument("--workflow-file", default="ci.yml")
    parser.add_argument("--artifact-name")
    parser.add_argument("--run-id")
    parser.add_argument("--p95-limit", type=float, default=8.0)
    parser.add_argument("--p99-limit", type=float, default=12.0)
    parser.add_argument("--throughput-limit", type=float, default=8.0)
    parser.add_argument("--strict-missing-baseline", action="store_true")
    parser.add_argument("--output-json")
    parser.add_argument("--output-markdown")
    parser.add_argument("--summary-path")
    parser.add_argument("--run-tests", action="store_true")
    args = parser.parse_args()

    if args.run_tests:
        return run_tests()

    if not args.lane:
        raise SystemExit("--lane is required unless --run-tests is set")

    if not args.current and not args.current_glob:
        raise SystemExit("Either --current or --current-glob is required")

    current_path = args.current
    if not current_path:
        candidates = sorted(glob.glob(args.current_glob or ""), key=os.path.getmtime)
        if not candidates:
            raise SystemExit("No current report found")
        current_path = candidates[-1]

    current_report = load_report(current_path)

    previous_report = None
    baseline_run_id = None
    if args.previous:
        previous_report = load_report(args.previous)
    elif args.repo and args.artifact_name and args.run_id:
        baseline_path, baseline_run_id = fetch_previous_report(
            args.repo, args.workflow_file, args.artifact_name, args.run_id
        )
        if baseline_path:
            previous_report = load_report(baseline_path)

    result = evaluate_gate(
        lane=args.lane,
        current_report=current_report,
        previous_report=previous_report,
        p95_regression_limit_pct=args.p95_limit,
        p99_regression_limit_pct=args.p99_limit,
        throughput_regression_limit_pct=args.throughput_limit,
        strict_missing_baseline=args.strict_missing_baseline,
    )
    if baseline_run_id:
        result["baseline_run_id"] = baseline_run_id

    markdown = format_markdown(result, baseline_run_id)

    if args.output_json:
        Path(args.output_json).write_text(json.dumps(result, indent=2), encoding="utf-8")
    if args.output_markdown:
        Path(args.output_markdown).write_text(markdown, encoding="utf-8")
    if args.summary_path:
        with open(args.summary_path, "a", encoding="utf-8") as handle:
            handle.write(markdown + "\n")

    print(markdown)
    return 0 if result["status"] == "pass" else 1


class BenchmarkRegressionGateTests(unittest.TestCase):
    def _report(self, p95: float, p99: float, throughput: float, perf: int = 5, skipped: int = 0) -> Dict[str, Any]:
        return {
            "summary": {
                "performance_scenarios": perf,
                "skipped_scenarios": skipped,
                "avg_latency_p95_ratio": p95,
                "avg_latency_p99_ratio": p99,
                "avg_throughput_ratio": throughput,
            }
        }

    def test_pass_case(self) -> None:
        current = self._report(1.02, 1.03, 1.01)
        baseline = self._report(1.00, 1.00, 1.00)
        result = evaluate_gate("fair-low", current, baseline, 8.0, 12.0, 8.0, True)
        self.assertEqual(result["status"], "pass")

    def test_threshold_breach_fails(self) -> None:
        current = self._report(1.20, 1.20, 0.70)
        baseline = self._report(1.00, 1.00, 1.00)
        result = evaluate_gate("fair-low", current, baseline, 8.0, 12.0, 8.0, True)
        self.assertEqual(result["status"], "fail")
        self.assertGreaterEqual(len(result["failures"]), 1)

    def test_missing_baseline_fails_when_strict(self) -> None:
        current = self._report(1.00, 1.00, 1.00)
        result = evaluate_gate("fair-low", current, None, 8.0, 12.0, 8.0, True)
        self.assertEqual(result["status"], "fail")

    def test_skipped_only_fails(self) -> None:
        current = self._report(1.00, 1.00, 1.00, perf=3, skipped=3)
        baseline = self._report(1.00, 1.00, 1.00)
        result = evaluate_gate("fair-low", current, baseline, 8.0, 12.0, 8.0, True)
        self.assertEqual(result["status"], "fail")


def run_tests() -> int:
    suite = unittest.defaultTestLoader.loadTestsFromTestCase(BenchmarkRegressionGateTests)
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    return 0 if result.wasSuccessful() else 1


if __name__ == "__main__":
    raise SystemExit(main())
