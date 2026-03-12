#!/usr/bin/env python3

import json
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
MATRIX = ROOT / "tests" / "harness" / "service-matrix.json"
MANIFESTS = ROOT / "manifests" / "guided"
REPORT = ROOT / "artifacts" / "studio_guided_coverage.json"


def load_matrix_services() -> list[str]:
    payload = json.loads(MATRIX.read_text(encoding="utf-8"))
    return sorted(item["name"] for item in payload.get("services", []))


def load_manifest_services() -> list[str]:
    services = []
    for path in sorted(MANIFESTS.glob("*.guided.json")):
        payload = json.loads(path.read_text(encoding="utf-8"))
        services.append(payload.get("service", path.stem.replace(".guided", "")))
    return sorted(services)


def main() -> int:
    matrix_services = load_matrix_services()
    manifest_services = load_manifest_services()

    missing = sorted(set(matrix_services) - set(manifest_services))
    extra = sorted(set(manifest_services) - set(matrix_services))
    report = {
        "matrix_services": matrix_services,
        "manifest_services": manifest_services,
        "missing_services": missing,
        "extra_services": extra,
        "ok": not missing,
    }

    REPORT.parent.mkdir(parents=True, exist_ok=True)
    REPORT.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")

    print(json.dumps(report, indent=2))
    return 0 if not missing else 1


if __name__ == "__main__":
    raise SystemExit(main())
