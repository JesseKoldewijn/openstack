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
    lane: str,
) -> tuple[Optional[Dict[str, Any]], Optional[str], Dict[str, Any]]:
    diagnostics: Dict[str, Any] = {
        "source": "github-actions-artifact",
        "workflow_file": workflow_file,
        "artifact_name": artifact_name,
        "current_run_id": current_run_id,
        "checked_run_ids": [],
        "failure_reason": None,
        "gh_token_present": bool(os.environ.get("GH_TOKEN")),
    }

    if not os.environ.get("GH_TOKEN"):
        diagnostics["failure_reason"] = "missing_gh_token"
        return None, None, diagnostics

    try:
        runs_data = run_json(
            [
                "gh",
                "api",
                f"repos/{repo}/actions/workflows/{workflow_file}/runs?status=success&per_page=30",
            ]
        )
    except Exception:
        diagnostics["failure_reason"] = "github_api_query_failed"
        return None, None, diagnostics

    for run in runs_data.get("workflow_runs", []):
        run_id = str(run.get("id"))
        if not run_id or run_id == current_run_id:
            continue
        diagnostics["checked_run_ids"].append(run_id)

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
            selected_report = select_baseline_report_path(reports, lane)
            if selected_report:
                diagnostics["failure_reason"] = None
                diagnostics["resolved_run_id"] = run_id
                return load_report(selected_report), run_id, diagnostics
        except Exception:
            pass
        finally:
            shutil.rmtree(temp_dir, ignore_errors=True)

    diagnostics["failure_reason"] = "baseline_artifact_not_found"
    return None, None, diagnostics


def load_report(path: str) -> Dict[str, Any]:
    with open(path, "r", encoding="utf-8") as handle:
        return json.load(handle)


def select_baseline_report_path(candidates: list[str], lane: str) -> Optional[str]:
    non_gate_reports = [
        path for path in candidates if not os.path.basename(path).startswith("benchmark-gate-")
    ]
    lane_reports = [
        path for path in non_gate_reports if os.path.basename(path).startswith(f"{lane}-")
    ]
    pool = lane_reports or non_gate_reports
    if not pool:
        return None
    return max(pool, key=os.path.getmtime)


def lane_metrics(report: Dict[str, Any]) -> Dict[str, Optional[float]]:
    summary = report.get("summary", {})
    return {
        "p95": summary.get("avg_latency_p95_ratio"),
        "p99": summary.get("avg_latency_p99_ratio"),
        "throughput": summary.get("avg_throughput_ratio"),
    }


def classify_breach(kind: str) -> Optional[str]:
    if kind == "missing_service_class":
        return "missing_service_class"
    if kind == "mode_mismatch":
        return "mode_mismatch"
    if kind.startswith("class-envelope-"):
        return "class_envelope_breach"
    return None


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
    runtime = current_report.get("runtime", {})
    perf_scenarios = int(summary.get("performance_scenarios", 0))
    valid_perf_scenarios = int(summary.get("valid_performance_scenarios", perf_scenarios))
    invalid_perf_scenarios = int(summary.get("invalid_performance_scenarios", 0))
    skipped_scenarios = int(summary.get("skipped_scenarios", 0))
    openstack_error_count = int(summary.get("openstack_error_count", 0))
    localstack_error_count = int(summary.get("localstack_error_count", 0))
    invalid_reasons = [str(reason) for reason in summary.get("invalid_reasons", []) if reason]
    per_service = summary.get("per_service", {})

    failures: list[str] = []
    checks: list[Dict[str, Any]] = []
    skipped_checks: list[Dict[str, str]] = []
    failure_category = None

    if perf_scenarios == 0:
        failures.append(f"lane {lane}: no performance scenarios in current report")
        failure_category = failure_category or "data_quality_missing_performance"
    if valid_perf_scenarios == 0:
        failures.append(f"lane {lane}: no valid performance scenarios in current report")
        failure_category = failure_category or "data_quality_no_valid_performance"
    if perf_scenarios > 0 and skipped_scenarios >= perf_scenarios:
        failures.append(f"lane {lane}: all performance scenarios are skipped")
        failure_category = failure_category or "data_quality_all_skipped"

    runtime_mode_equivalent = runtime.get("persistence_mode_equivalent")
    if runtime_mode_equivalent is False:
        failures.append(f"lane {lane}: benchmark run uses non-equivalent persistence modes")
        failure_category = failure_category or "mode_mismatch"

    service_class_failures = []
    missing_class_failures = 0
    envelope_breach_failures = 0
    service_diagnostics = []
    for service, service_summary in per_service.items():
        service_class = service_summary.get("service_execution_class")
        service_durability = service_summary.get("service_durability_class")
        breaches = service_summary.get("class_envelope_breaches", [])
        service_diagnostics.append(
            {
                "service": service,
                "service_execution_class": service_class,
                "service_durability_class": service_durability,
                "class_envelope_breaches": breaches,
            }
        )
        if not service_class:
            service_class_failures.append(f"lane {lane}: service '{service}' missing service class")
            missing_class_failures += 1
        for breach in breaches:
            service_class_failures.append(
                f"lane {lane}: service '{service}' class envelope breach ({breach})"
            )
            envelope_breach_failures += 1
    if service_class_failures:
        failures.extend(service_class_failures)
        if missing_class_failures > 0 and envelope_breach_failures == 0:
            failure_category = failure_category or "missing_service_class"
        else:
            failure_category = failure_category or "class_envelope_breach"

    mismatch_found = any(classify_breach(reason.split(":", 1)[1] if ":" in reason else reason) == "mode_mismatch" for reason in invalid_reasons)
    if mismatch_found:
        failures.append(f"lane {lane}: mode mismatch detected in benchmark validity reasons")
        failure_category = failure_category or "mode_mismatch"

    persistence_quality_found = any(
        "persistence_" in reason or "recovery" in reason or "durability" in reason
        for reason in invalid_reasons
    )
    if persistence_quality_found:
        failures.append(f"lane {lane}: persistence-quality failure detected in scenario validity")
        failure_category = failure_category or "persistence_quality_failure"

    current = lane_metrics(current_report)
    previous = lane_metrics(previous_report) if previous_report else None

    if previous is None:
        if strict_missing_baseline:
            failures.append(
                f"lane {lane}: baseline missing for required lane (seed a successful baseline run first)"
            )
            failure_category = failure_category or "baseline_missing"
    else:
        latency_checks = [
            ("p95", p95_regression_limit_pct),
            ("p99", p99_regression_limit_pct),
        ]
        for metric, threshold in latency_checks:
            c = current.get(metric)
            p = previous.get(metric)
            if c is None:
                failures.append(f"lane {lane}: missing metric '{metric}' in current report")
                failure_category = failure_category or "data_quality_missing_metric"
                continue
            if p is None:
                skipped_checks.append(
                    {
                        "metric": metric,
                        "reason": "baseline_missing_metric",
                    }
                )
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
                failure_category = failure_category or "threshold_breach"

        c = current.get("throughput")
        p = previous.get("throughput")
        if c is None:
            failures.append(f"lane {lane}: missing metric 'throughput' in current report")
            failure_category = failure_category or "data_quality_missing_metric"
        elif p is None:
            skipped_checks.append(
                {
                    "metric": "throughput",
                    "reason": "baseline_missing_metric",
                }
            )
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
                failure_category = failure_category or "threshold_breach"

    return {
        "lane": lane,
        "status": "pass" if not failures else "fail",
        "performance_scenarios": perf_scenarios,
        "valid_performance_scenarios": valid_perf_scenarios,
        "invalid_performance_scenarios": invalid_perf_scenarios,
        "skipped_scenarios": skipped_scenarios,
        "openstack_error_count": openstack_error_count,
        "localstack_error_count": localstack_error_count,
        "invalid_reasons": invalid_reasons,
        "service_class_failures": service_class_failures,
        "service_diagnostics": service_diagnostics,
        "runtime_mode_equivalent": runtime_mode_equivalent,
        "current": current,
        "baseline": previous,
        "checks": checks,
        "skipped_checks": skipped_checks,
        "baseline_incompatible_metrics": sorted(
            {entry["metric"] for entry in skipped_checks if entry.get("reason") == "baseline_missing_metric"}
        ),
        "failures": failures,
        "thresholds": {
            "p95_regression_limit_pct": p95_regression_limit_pct,
            "p99_regression_limit_pct": p99_regression_limit_pct,
            "throughput_regression_limit_pct": throughput_regression_limit_pct,
        },
        "failure_category": failure_category,
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

    service_diagnostics = result.get("service_diagnostics", [])
    if service_diagnostics:
        lines.extend(["", "Service diagnostics:"])
        for entry in sorted(service_diagnostics, key=lambda e: e.get("service", "")):
            lines.append(
                "- {service}: class={clazz}, durability={durability}, envelope_breaches={breaches}".format(
                    service=entry.get("service", "unknown"),
                    clazz=entry.get("service_execution_class", "n/a"),
                    durability=entry.get("service_durability_class", "n/a"),
                    breaches=len(entry.get("class_envelope_breaches", []) or []),
                )
            )
    skipped_checks = result.get("skipped_checks", [])
    if skipped_checks:
        lines.extend(["", "Skipped checks:"])
        for skipped in skipped_checks:
            metric = skipped.get("metric", "unknown")
            reason = skipped.get("reason", "n/a")
            if reason == "baseline_missing_metric":
                lines.append(
                    f"- lane {lane}: skipped metric '{metric}' check because baseline report is missing this metric"
                )
            else:
                lines.append(f"- lane {lane}: skipped metric '{metric}' check ({reason})")
    if result.get("failure_category"):
        lines.extend(["", f"Failure category: `{result['failure_category']}`"])

    remediation = {
        "mode_mismatch": "align PARITY_OPENSTACK_PERSISTENCE_MODE and PARITY_LOCALSTACK_PERSISTENCE_MODE for the lane",
        "class_envelope_breach": "reduce latency/memory or increase throughput for breached services, then re-run required lane",
        "persistence_quality_failure": "fix persistence lifecycle behavior (save/load/recovery) for failing scenarios and re-run parity + benchmark lanes",
        "missing_service_class": "define service class for unclassified service in benchmark classification map",
        "baseline_missing": "seed a successful baseline run for this lane",
    }
    category = result.get("failure_category")
    if category in remediation:
        lines.extend(["", f"Remediation: {remediation[category]}"])

    if result.get("failure_category") == "data_quality_no_valid_performance":
        lines.extend(
            [
                "",
                "Signal quality diagnostics:",
                f"- performance_scenarios: {result.get('performance_scenarios', 0)}",
                f"- valid_performance_scenarios: {result.get('valid_performance_scenarios', 0)}",
                f"- invalid_performance_scenarios: {result.get('invalid_performance_scenarios', 0)}",
                f"- skipped_scenarios: {result.get('skipped_scenarios', 0)}",
                f"- openstack_error_count: {result.get('openstack_error_count', 0)}",
                f"- localstack_error_count: {result.get('localstack_error_count', 0)}",
            ]
        )
        invalid_reasons = result.get("invalid_reasons", [])
        if invalid_reasons:
            lines.append("- sample_invalid_reasons:")
            for reason in invalid_reasons[:5]:
                lines.append(f"- invalid_reason: {reason}")

    diagnostics = result.get("diagnostics")
    if diagnostics:
        lines.extend(["", "Diagnostics:"])
        lines.append(f"- source: {diagnostics.get('source', 'n/a')}")
        lines.append(f"- gh_token_present: {diagnostics.get('gh_token_present', False)}")
        if diagnostics.get("failure_reason"):
            lines.append(f"- baseline_lookup_failure: {diagnostics.get('failure_reason')}")
        checked = diagnostics.get("checked_run_ids", [])
        if checked:
            lines.append(f"- checked_run_ids: {', '.join(checked)}")
        lines.extend(
            [
                "- remediation: set GH_TOKEN in workflow env, verify artifact name/workflow file, and seed a successful baseline run",
            ]
        )

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
    parser.add_argument("--warning-mode", action="store_true")
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
    diagnostics: Dict[str, Any] = {
        "source": "local_or_explicit_previous",
        "gh_token_present": bool(os.environ.get("GH_TOKEN")),
        "failure_reason": None,
        "checked_run_ids": [],
    }
    if args.previous:
        previous_report = load_report(args.previous)
    elif args.repo and args.artifact_name and args.run_id:
        baseline_report, baseline_run_id, diagnostics = fetch_previous_report(
            args.repo,
            args.workflow_file,
            args.artifact_name,
            args.run_id,
            args.lane,
        )
        if baseline_report is not None:
            previous_report = baseline_report

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
    result["diagnostics"] = diagnostics
    if diagnostics.get("failure_reason") == "missing_gh_token" and result["failure_category"] == "baseline_missing":
        result["failure_category"] = "token_missing"

    if args.warning_mode and result["status"] == "fail":
        result["status"] = "warning"
        result["warning_mode"] = True

    markdown = format_markdown(result, baseline_run_id)

    if args.output_json:
        Path(args.output_json).write_text(json.dumps(result, indent=2), encoding="utf-8")
    if args.output_markdown:
        Path(args.output_markdown).write_text(markdown, encoding="utf-8")
    if args.summary_path:
        with open(args.summary_path, "a", encoding="utf-8") as handle:
            handle.write(markdown + "\n")

    print(markdown)
    return 0 if result["status"] in ("pass", "warning") else 1


class BenchmarkRegressionGateTests(unittest.TestCase):
    def _report(self, p95: float, p99: float, throughput: float, perf: int = 5, skipped: int = 0) -> Dict[str, Any]:
        return {
            "summary": {
                "performance_scenarios": perf,
                "valid_performance_scenarios": perf,
                "invalid_performance_scenarios": 0,
                "skipped_scenarios": skipped,
                "openstack_error_count": 0,
                "localstack_error_count": 0,
                "invalid_reasons": [],
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

    def test_warning_mode_can_downgrade_fail_status(self) -> None:
        current = self._report(1.20, 1.20, 0.70)
        baseline = self._report(1.00, 1.00, 1.00)
        result = evaluate_gate("fair-low", current, baseline, 8.0, 12.0, 8.0, True)
        self.assertEqual(result["status"], "fail")

        result["status"] = "warning"
        self.assertEqual(result["status"], "warning")

    def test_missing_baseline_fails_when_strict(self) -> None:
        current = self._report(1.00, 1.00, 1.00)
        result = evaluate_gate("fair-low", current, None, 8.0, 12.0, 8.0, True)
        self.assertEqual(result["status"], "fail")

    def test_skipped_only_fails(self) -> None:
        current = self._report(1.00, 1.00, 1.00, perf=3, skipped=3)
        baseline = self._report(1.00, 1.00, 1.00)
        result = evaluate_gate("fair-low", current, baseline, 8.0, 12.0, 8.0, True)
        self.assertEqual(result["status"], "fail")

    def test_no_valid_performance_fails(self) -> None:
        current = self._report(1.00, 1.00, 1.00, perf=3, skipped=0)
        current["summary"]["valid_performance_scenarios"] = 0
        baseline = self._report(1.00, 1.00, 1.00)
        result = evaluate_gate("fair-low", current, baseline, 8.0, 12.0, 8.0, True)
        self.assertEqual(result["status"], "fail")
        self.assertEqual(result["failure_category"], "data_quality_no_valid_performance")

    def test_baseline_missing_metric_skips_check(self) -> None:
        current = self._report(1.02, 1.03, 1.01)
        baseline = self._report(1.00, 1.00, 1.00)
        baseline["summary"].pop("avg_latency_p99_ratio", None)

        result = evaluate_gate("fair-low-core", current, baseline, 8.0, 12.0, 8.0, True)

        self.assertEqual(result["status"], "pass")
        self.assertIn("p99", result.get("baseline_incompatible_metrics", []))
        self.assertEqual(
            [check["metric"] for check in result.get("checks", [])],
            ["p95", "throughput"],
        )

    def test_current_missing_metric_fails(self) -> None:
        current = self._report(1.02, 1.03, 1.01)
        baseline = self._report(1.00, 1.00, 1.00)
        current["summary"].pop("avg_latency_p99_ratio", None)

        result = evaluate_gate("fair-low-core", current, baseline, 8.0, 12.0, 8.0, True)

        self.assertEqual(result["status"], "fail")
        self.assertEqual(result["failure_category"], "data_quality_missing_metric")
        self.assertIn("missing metric 'p99' in current report", "\n".join(result["failures"]))

    def test_missing_gh_token_diagnostic(self) -> None:
        previous_token = os.environ.pop("GH_TOKEN", None)
        try:
            _path, _run_id, diagnostics = fetch_previous_report(
                "owner/repo", "ci.yml", "artifact", "123", "fair-low-core"
            )
            self.assertEqual(diagnostics.get("failure_reason"), "missing_gh_token")
            self.assertFalse(diagnostics.get("gh_token_present"))
        finally:
            if previous_token is not None:
                os.environ["GH_TOKEN"] = previous_token

    def test_select_baseline_prefers_lane_report_over_gate_json(self) -> None:
        with tempfile.TemporaryDirectory(prefix="bench-gate-select-") as temp_dir:
            lane_report = Path(temp_dir, "fair-low-core-20260310164402.json")
            gate_report = Path(temp_dir, "benchmark-gate-fair-low-core-123456.json")
            other_lane = Path(temp_dir, "fair-medium-core-20260310164403.json")
            lane_report.write_text("{}", encoding="utf-8")
            gate_report.write_text("{}", encoding="utf-8")
            other_lane.write_text("{}", encoding="utf-8")

            selected = select_baseline_report_path(
                [str(lane_report), str(gate_report), str(other_lane)], "fair-low-core"
            )
            self.assertEqual(selected, str(lane_report))

    def test_data_quality_markdown_includes_signal_details(self) -> None:
        current = self._report(1.00, 1.00, 1.00, perf=2, skipped=0)
        current["summary"]["valid_performance_scenarios"] = 0
        current["summary"]["invalid_performance_scenarios"] = 2
        current["summary"]["openstack_error_count"] = 24
        current["summary"]["localstack_error_count"] = 24
        current["summary"]["invalid_reasons"] = [
            "s3-core-list: insufficient cross-target successful operations",
            "sns-core-list: all operations failed",
        ]
        baseline = self._report(1.00, 1.00, 1.00)

        result = evaluate_gate("fair-low-core", current, baseline, 8.0, 12.0, 8.0, True)
        markdown = format_markdown(result, "123")

        self.assertIn("Signal quality diagnostics:", markdown)
        self.assertIn("- valid_performance_scenarios: 0", markdown)
        self.assertIn("- invalid_performance_scenarios: 2", markdown)
        self.assertIn("- openstack_error_count: 24", markdown)
        self.assertIn("sample_invalid_reasons", markdown)

    def test_mode_mismatch_fails_fast(self) -> None:
        current = self._report(1.00, 1.00, 1.00)
        current["runtime"] = {"persistence_mode_equivalent": False}
        baseline = self._report(1.00, 1.00, 1.00)

        result = evaluate_gate("fair-low-core", current, baseline, 8.0, 12.0, 8.0, True)

        self.assertEqual(result["status"], "fail")
        self.assertEqual(result["failure_category"], "mode_mismatch")

    def test_missing_service_class_sets_failure_category(self) -> None:
        current = self._report(1.00, 1.00, 1.00)
        current["summary"]["per_service"] = {
            "s3": {
                "service_execution_class": None,
                "service_durability_class": "durable",
                "class_envelope_breaches": [],
            }
        }
        baseline = self._report(1.00, 1.00, 1.00)

        result = evaluate_gate("fair-low-core", current, baseline, 8.0, 12.0, 8.0, True)

        self.assertEqual(result["status"], "fail")
        self.assertEqual(result["failure_category"], "missing_service_class")

    def test_class_envelope_breach_sets_failure_category(self) -> None:
        current = self._report(1.00, 1.00, 1.00)
        current["summary"]["per_service"] = {
            "s3": {
                "service_execution_class": "in-proc-stateful",
                "service_durability_class": "durable",
                "class_envelope_breaches": ["s3-put:class-envelope-latency-p95-breach:1.23"],
            }
        }
        baseline = self._report(1.00, 1.00, 1.00)

        result = evaluate_gate("fair-low-core", current, baseline, 8.0, 12.0, 8.0, True)

        self.assertEqual(result["status"], "fail")
        self.assertEqual(result["failure_category"], "class_envelope_breach")


def run_tests() -> int:
    suite = unittest.defaultTestLoader.loadTestsFromTestCase(BenchmarkRegressionGateTests)
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    return 0 if result.wasSuccessful() else 1


if __name__ == "__main__":
    raise SystemExit(main())
