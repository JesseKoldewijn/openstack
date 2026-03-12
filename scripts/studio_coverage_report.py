#!/usr/bin/env python3
"""Generate Studio service coverage summary for CI."""

import json
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
INPUT = ROOT / "crates" / "internal-api" / "src" / "studio.rs"
OUTPUT = ROOT / "artifacts" / "studio_coverage_report.json"


def main() -> int:
    text = INPUT.read_text(encoding="utf-8")
    guided = []
    for line in text.splitlines():
        line = line.strip()
        if "=> \"guided\"" not in line:
            continue
        for segment in line.split("|"):
            segment = segment.strip()
            if segment.startswith('"'):
                parts = segment.split('"')
                if len(parts) >= 3:
                    guided.append(parts[1])

    report = {
        "guided": sorted(guided),
        "raw_or_other": "all other registered services",
        "source": str(INPUT.relative_to(ROOT)),
    }

    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
