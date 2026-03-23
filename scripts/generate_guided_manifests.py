#!/usr/bin/env python3

import json
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
SERVICES_DIR = ROOT / "crates" / "services"
MANIFESTS_DIR = ROOT / "manifests" / "guided"


def protocol_for(service: str) -> str:
    query = {"sqs", "sns", "iam", "sts", "cloudformation"}
    json_target = {
        "dynamodb",
        "kinesis",
        "firehose",
        "secretsmanager",
        "ssm",
        "kms",
        "cloudwatch",
    }
    if service == "s3":
        return "rest_xml"
    if service in query:
        return "query"
    if service in json_target:
        return "json_target"
    return "rest_json"


def build_manifest(service: str) -> dict:
    protocol = protocol_for(service)
    op = {"method": "POST", "path": "/", "body": "{}"}
    if protocol == "rest_xml":
        op = {"method": "PUT", "path": "/{{inputs.resource_name}}"}
    elif protocol == "query":
        op = {
            "method": "POST",
            "path": "/",
            "body": f"Action=List{service.capitalize()}&Version=2012-11-05",
        }
    elif protocol == "json_target":
        op = {
            "method": "POST",
            "path": "/",
            "headers": {"x-amz-target": f"{service}.List"},
            "body": "{}",
        }

    flow = {
        "id": "l1-basic",
        "level": "L1",
        "steps": [
            {
                "id": "create",
                "title": "Create resource",
                "operation": op,
                "captures": [
                    {"name": "resource_name", "source": "inputs.resource_name"}
                ],
                "assertions": [
                    {"kind": "status", "target": "status", "expected": "200"}
                ],
            }
        ],
        "cleanup": [
            {
                "id": "cleanup",
                "title": "Cleanup",
                "operation": {"method": "DELETE", "path": "/{{captures.resource_name}}"},
                "assertions": [
                    {"kind": "status", "target": "status", "expected": "200"}
                ],
            }
        ],
    }
    return {
        "schemaVersion": "1.2",
        "service": service,
        "protocol": protocol,
        "inputs": [
            {
                "name": "resource_name",
                "type": "string",
                "required": True,
                "description": f"Resource name for {service}",
            }
        ],
        "flows": [flow],
    }


def main() -> int:
    MANIFESTS_DIR.mkdir(parents=True, exist_ok=True)
    services = sorted(
        path.name
        for path in SERVICES_DIR.iterdir()
        if path.is_dir() and not path.name.startswith(".")
    )
    for service in services:
        output = MANIFESTS_DIR / f"{service}.guided.json"
        if output.exists():
            continue
        output.write_text(json.dumps(build_manifest(service), indent=2) + "\n", encoding="utf-8")
        print(f"wrote {output.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
