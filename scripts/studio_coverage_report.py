#!/usr/bin/env python3
"""Generate Studio guided-flow coverage summary for CI."""

import json
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
INPUT = ROOT / "manifests" / "guided"
OUTPUT = ROOT / "artifacts" / "studio_coverage_report.json"


def main() -> int:
    guided = []
    for path in sorted(INPUT.glob("*.guided.json")):
        payload = json.loads(path.read_text(encoding="utf-8"))
        guided.append(
            {
                "service": payload.get("service", path.stem.replace(".guided", "")),
                "schemaVersion": payload.get("schemaVersion"),
                "protocol": payload.get("protocol"),
                "flowCount": len(payload.get("flows", [])),
            }
        )

    report = {
        "guided": guided,
        "total_guided_services": len(guided),
        "source": str(INPUT.relative_to(ROOT)),
    }

    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
