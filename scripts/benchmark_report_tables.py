#!/usr/bin/env python3

import argparse
import glob
import json
import os
import shutil
import subprocess
import tempfile
from pathlib import Path
from typing import Any, Dict, Optional


METRIC_KEYS = [
    ("latency_p50_ms", "Latency p50 (ms)", False),
    ("latency_p95_ms", "Latency p95 (ms)", False),
    ("latency_p99_ms", "Latency p99 (ms)", False),
    ("throughput_ops_per_sec", "Throughput (ops/s)", True),
]


def load_report(path: str) -> Dict[str, Any]:
    with open(path, "r", encoding="utf-8") as handle:
        return json.load(handle)


def weighted_value(report: Dict[str, Any], target: str, key: str) -> Optional[float]:
    weighted_sum = 0.0
    total_weight = 0.0
    for result in report.get("results", []):
        if result.get("scenario_class") != "performance":
            continue
        if result.get("skipped"):
            continue
        metrics = result.get(target, {}).get("metrics", {})
        value = metrics.get(key)
        weight = float(metrics.get("operation_count", 0))
        if value is None or weight <= 0:
            continue
        weighted_sum += float(value) * weight
        total_weight += weight
    if total_weight <= 0:
        return None
    return weighted_sum / total_weight


def fmt_num(value: Optional[float], places: int = 3) -> str:
    if value is None:
        return "n/a"
    return f"{value:.{places}f}"


def service_ratio(value: Optional[float]) -> str:
    if value is None:
        return "n/a"
    return f"{value:.3f}"


def pct_delta(current: Optional[float], previous: Optional[float]) -> Optional[float]:
    if current is None or previous is None:
        return None
    if abs(previous) <= 1e-12:
        return None
    return ((current - previous) / previous) * 100.0


def classify_delta(delta: Optional[float], higher_is_better: bool) -> str:
    if delta is None:
        return "n/a"
    threshold = 5.0
    if higher_is_better:
        if delta > threshold:
            return "improved"
        if delta < -threshold:
            return "regressed"
        return "stable"
    if delta < -threshold:
        return "improved"
    if delta > threshold:
        return "regressed"
    return "stable"


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

        temp_dir = tempfile.mkdtemp(prefix="benchmark-baseline-")
        try:
            subprocess.check_call(
                [
                    "gh",
                    "run",
                    "download",
                    run_id,
                    "-n",
                    artifact_name,
                    "-D",
                    temp_dir,
                ]
            )
            reports = sorted(glob.glob(f"{temp_dir}/**/*.json", recursive=True), key=os.path.getmtime)
            if reports:
                return reports[-1], run_id
        except Exception:
            pass
        finally:
            shutil.rmtree(temp_dir, ignore_errors=True)

    return None, None


def build_markdown(
    current_report: Dict[str, Any],
    title: str,
    previous_report: Optional[Dict[str, Any]],
    previous_run_id: Optional[str],
) -> str:
    lines = [f"## {title}", "", "### Current: OpenStack vs LocalStack", ""]

    summary = current_report.get("summary", {})
    lines.extend(
        [
            f"Scenarios: total={summary.get('total_scenarios', 'n/a')}, performance={summary.get('performance_scenarios', 'n/a')}, valid-performance={summary.get('valid_performance_scenarios', 'n/a')}, invalid-performance={summary.get('invalid_performance_scenarios', 'n/a')}, coverage={summary.get('coverage_scenarios', 'n/a')}, skipped={summary.get('skipped_scenarios', 'n/a')}",
            f"Lane interpretable: `{summary.get('lane_interpretable', False)}`",
            "",
        ]
    )

    memory = current_report.get("memory_summary")
    if memory:
        os_bytes = memory.get("openstack_rss_bytes")
        ls_bytes = memory.get("localstack_rss_bytes")
        ratio = memory.get("rss_ratio_openstack_over_localstack")

        def _to_mb(v: Optional[float]) -> str:
            if v is None:
                return "n/a"
            return f"{(float(v)/(1024*1024)):.2f}"

        lines.extend(
            [
                "Memory summary (container RSS):",
                f"- OpenStack RSS (MB): {_to_mb(os_bytes)}",
                f"- LocalStack RSS (MB): {_to_mb(ls_bytes)}",
                f"- RSS ratio (OS/LS): {fmt_num(ratio)}",
                "",
            ]
        )

    if summary.get("lane_interpretable", False) is False:
        lines.extend(
            [
                "⚠️ Performance lane is non-interpretable; trend/gate conclusions should not be treated as optimization signal.",
                "",
            ]
        )

    invalid_reasons = summary.get("invalid_reasons", [])
    if invalid_reasons:
        lines.extend(["Invalid scenario exclusions:"])
        for reason in invalid_reasons:
            lines.append(f"- {reason}")
        lines.append("")

    lines.extend(
        [
            "| Metric (performance only) | OpenStack (weighted) | LocalStack (weighted) | OS/LS ratio |",
            "|---|---:|---:|---:|",
        ]
    )

    for key, label, _ in METRIC_KEYS:
        os_value = weighted_value(current_report, "openstack", key)
        ls_value = weighted_value(current_report, "localstack", key)
        ratio = None
        if ls_value is not None and abs(ls_value) > 1e-12 and os_value is not None:
            ratio = os_value / ls_value
        lines.append(f"| {label} | {fmt_num(os_value)} | {fmt_num(ls_value)} | {fmt_num(ratio)} |")

    per_service = current_report.get("summary", {}).get("per_service", {})
    if per_service:
        lines.extend([
            "",
            "### Per-Service Comparison (OS/LS)",
            "",
            "| Service | Scenarios | Skipped | p95 ratio | p99 ratio | Throughput ratio |",
            "|---|---:|---:|---:|---:|---:|",
        ])
        for service in sorted(per_service.keys()):
            entry = per_service[service]
            lines.append(
                "| {service} | {scenarios} | {skipped} | {p95} | {p99} | {tp} |".format(
                    service=service,
                    scenarios=entry.get("total_scenarios", 0),
                    skipped=entry.get("skipped_scenarios", 0),
                    p95=service_ratio(entry.get("avg_latency_p95_ratio")),
                    p99=service_ratio(entry.get("avg_latency_p99_ratio")),
                    tp=service_ratio(entry.get("avg_throughput_ratio")),
                )
            )

    if previous_report is None:
        lines.extend(["", "No previous successful run baseline found for this lane."])
        return "\n".join(lines) + "\n"

    lines.extend(["", "### Trend: Current vs Previous Successful Run", ""])
    lines.extend(
        [
            "| Metric | Current OS/LS ratio | Previous OS/LS ratio | Delta % | Status |",
            "|---|---:|---:|---:|---|",
        ]
    )

    for key, label, higher_is_better in METRIC_KEYS:
        current_os = weighted_value(current_report, "openstack", key)
        current_ls = weighted_value(current_report, "localstack", key)
        prev_os = weighted_value(previous_report, "openstack", key)
        prev_ls = weighted_value(previous_report, "localstack", key)

        current_ratio = None
        previous_ratio = None
        if current_os is not None and current_ls is not None and abs(current_ls) > 1e-12:
            current_ratio = current_os / current_ls
        if prev_os is not None and prev_ls is not None and abs(prev_ls) > 1e-12:
            previous_ratio = prev_os / prev_ls

        delta = pct_delta(current_ratio, previous_ratio)
        status = classify_delta(delta, higher_is_better)
        lines.append(
            f"| {label} | {fmt_num(current_ratio)} | {fmt_num(previous_ratio)} | {fmt_num(delta)}% | {status} |"
        )

    if previous_run_id:
        lines.extend(["", f"Baseline run id: `{previous_run_id}`"])

    return "\n".join(lines) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description="Generate benchmark markdown tables")
    parser.add_argument("--current", help="Path to current benchmark report json")
    parser.add_argument("--current-glob", help="Glob pattern to locate latest current report")
    parser.add_argument("--title", required=True, help="Markdown section title")
    parser.add_argument("--repo", help="GitHub repo owner/name")
    parser.add_argument("--workflow-file", default="ci.yml", help="Workflow file name")
    parser.add_argument("--artifact-name", help="Artifact name for baseline lookup")
    parser.add_argument("--run-id", help="Current run id")
    parser.add_argument("--summary-path", help="Path to append markdown summary")
    parser.add_argument("--output-path", help="Path to write markdown")
    args = parser.parse_args()

    if not args.current and not args.current_glob:
        raise SystemExit("Either --current or --current-glob is required")

    current_path = args.current
    if not current_path:
        candidates = sorted(glob.glob(args.current_glob or ""), key=os.path.getmtime)
        if not candidates:
            raise SystemExit(0)
        current_path = candidates[-1]

    current = load_report(current_path)

    previous = None
    previous_run_id = None
    if args.repo and args.artifact_name and args.run_id:
        baseline_path, previous_run_id = fetch_previous_report(
            repo=args.repo,
            workflow_file=args.workflow_file,
            artifact_name=args.artifact_name,
            current_run_id=args.run_id,
        )
        if baseline_path:
            previous = load_report(baseline_path)

    markdown = build_markdown(current, args.title, previous, previous_run_id)

    if args.summary_path:
        with open(args.summary_path, "a", encoding="utf-8") as summary:
            summary.write(markdown + "\n")
    if args.output_path:
        Path(args.output_path).write_text(markdown, encoding="utf-8")
    else:
        print(markdown)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
