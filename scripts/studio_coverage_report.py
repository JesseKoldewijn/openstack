#!/usr/bin/env python3
"""Generate Studio guided-flow coverage summary for CI."""

import json
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
INPUT = ROOT / "manifests" / "guided"
MATRIX = ROOT / "tests" / "harness" / "service-matrix.json"
OUTPUT = ROOT / "artifacts" / "studio_coverage_report.json"


def main() -> int:
    matrix_payload = json.loads(MATRIX.read_text(encoding="utf-8"))
    supported_services = sorted(item["name"] for item in matrix_payload.get("services", []))

    guided = []
    protocol_counts: dict[str, int] = {}
    for path in sorted(INPUT.glob("*.guided.json")):
        payload = json.loads(path.read_text(encoding="utf-8"))
        protocol = payload.get("protocol", "unknown")
        protocol_counts[protocol] = protocol_counts.get(protocol, 0) + 1
        guided.append(
            {
                "service": payload.get("service", path.stem.replace(".guided", "")),
                "schemaVersion": payload.get("schemaVersion"),
                "protocol": protocol,
                "flowCount": len(payload.get("flows", [])),
            }
        )

    guided_services = sorted(item["service"] for item in guided)
    missing_services = sorted(set(supported_services) - set(guided_services))

    report = {
        "guided": guided,
        "total_guided_services": len(guided),
        "total_supported_services": len(supported_services),
        "protocol_counts": protocol_counts,
        "missing_services": missing_services,
        "source": str(INPUT.relative_to(ROOT)),
    }

    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
