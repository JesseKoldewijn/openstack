#!/usr/bin/env python3

import json
import pathlib
import sys


def load_json(path: pathlib.Path):
    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def main() -> int:
    root = pathlib.Path(__file__).resolve().parents[1]

    services_dir = root / "crates" / "services"
    def normalize_service_name(name: str) -> str:
        if name == "eventbridge":
            return "events"
        if name == "stepfunctions":
            return "states"
        return name

    implemented = sorted(
        normalize_service_name(p.name)
        for p in services_dir.iterdir()
        if p.is_dir() and not p.name.startswith(".")
    )

    matrix_path = root / "tests" / "harness" / "service-matrix.json"
    matrix = load_json(matrix_path)
    matrix_services = sorted(item["name"] for item in matrix.get("services", []))

    parity_path = root / "tests" / "parity" / "scenarios" / "all-services-smoke.json"
    parity_services = sorted({item["service"] for item in load_json(parity_path)})

    benchmark_path = root / "tests" / "benchmark" / "scenarios" / "all-services-smoke.json"
    benchmark_services = sorted({item["service"] for item in load_json(benchmark_path)})

    manifests_dir = root / "manifests" / "guided"
    manifest_services = sorted(
        {
            load_json(path).get("service", path.stem.replace(".guided", ""))
            for path in manifests_dir.glob("*.guided.json")
        }
    )

    failures = []

    if implemented != matrix_services:
        failures.append(
            "service-matrix does not match implemented services\n"
            f"implemented={implemented}\n"
            f"matrix={matrix_services}"
        )

    missing_parity = sorted(set(matrix_services) - set(parity_services))
    if missing_parity:
        failures.append(f"parity all-services-smoke missing services: {missing_parity}")

    missing_benchmark = sorted(set(matrix_services) - set(benchmark_services))
    if missing_benchmark:
        failures.append(f"benchmark all-services-smoke missing services: {missing_benchmark}")

    missing_manifests = sorted(set(matrix_services) - set(manifest_services))
    if missing_manifests:
        failures.append(f"guided manifests missing services: {missing_manifests}")

    if failures:
        print("Coverage validation failed:\n")
        for failure in failures:
            print(f"- {failure}")
        return 1

    print("Coverage validation passed.")
    print(f"Implemented services: {len(implemented)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
