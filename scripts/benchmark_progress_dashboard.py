#!/usr/bin/env python3

import argparse
import glob
import json
import os
from pathlib import Path
from typing import Any, Dict, Optional


def latest_report(pattern: str) -> Optional[Dict[str, Any]]:
    matches = sorted(glob.glob(pattern), key=os.path.getmtime)
    if not matches:
        return None
    with open(matches[-1], "r", encoding="utf-8") as handle:
        return json.load(handle)


def fmt_ratio(value: Optional[float]) -> str:
    if value is None:
        return "n/a"
    return f"{value:.3f}"


def build_dashboard(report_dir: str, parity_dir: str) -> str:
    bench = latest_report(os.path.join(report_dir, "fair-medium-core-latest.json"))
    parity = latest_report(os.path.join(parity_dir, "all-services-smoke-latest.json"))

    lines = [
        "## Performance & Parity Dashboard",
        "",
        "| Metric | Value |",
        "|---|---|",
    ]

    if bench:
        summary = bench.get("summary", {})
        lines.append(f"| Benchmark profile | `{bench.get('profile', 'n/a')}` |")
        lines.append(f"| Lane interpretable | `{summary.get('lane_interpretable', False)}` |")
        lines.append(f"| Avg p95 ratio (OS/LS) | {fmt_ratio(summary.get('avg_latency_p95_ratio'))} |")
        lines.append(f"| Avg p99 ratio (OS/LS) | {fmt_ratio(summary.get('avg_latency_p99_ratio'))} |")
        lines.append(f"| Avg throughput ratio (OS/LS) | {fmt_ratio(summary.get('avg_throughput_ratio'))} |")
    else:
        lines.append("| Benchmark profile | n/a |")

    if parity:
        ps = parity.get("summary", {})
        total = ps.get("total_scenarios", 0)
        passed = ps.get("passed", 0)
        pass_rate = (passed / total) if total else 0.0
        lines.append(f"| Parity profile | `{parity.get('profile', 'n/a')}` |")
        lines.append(f"| Parity pass rate | {pass_rate:.3f} |")
        lines.append(f"| Parity accepted differences | {ps.get('accepted_differences', 0)} |")
    else:
        lines.append("| Parity profile | n/a |")

    if bench:
        lines.extend(["", "### Service-Class Snapshot", "", "| Service | Class | Durability | Envelope breaches |", "|---|---|---|---:|"])
        per_service = bench.get("summary", {}).get("per_service", {})
        for service in sorted(per_service.keys()):
            entry = per_service[service]
            lines.append(
                "| {service} | {clazz} | {durability} | {breaches} |".format(
                    service=service,
                    clazz=entry.get("service_execution_class", "n/a"),
                    durability=entry.get("service_durability_class", "n/a"),
                    breaches=len(entry.get("class_envelope_breaches", []) or []),
                )
            )

    return "\n".join(lines).strip() + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description="Build benchmark/parity dashboard")
    parser.add_argument("--report-dir", default="target/benchmark-reports")
    parser.add_argument("--parity-dir", default="target/parity-reports")
    parser.add_argument("--output-path")
    parser.add_argument("--summary-path")
    args = parser.parse_args()

    content = build_dashboard(args.report_dir, args.parity_dir)

    if args.output_path:
        Path(args.output_path).write_text(content, encoding="utf-8")
    else:
        print(content)

    if args.summary_path:
        with open(args.summary_path, "a", encoding="utf-8") as handle:
            handle.write(content + "\n")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
