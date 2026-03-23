# Studio Guided Manifest Authoring Guide

This guide defines how to author, validate, and maintain guided flow manifests.

## Schema reference

- Schema file: `crates/studio-ui/schemas/studio-guided-flow.manifest.v1.schema.json`
- Current schema version: `1.2`
- Allowed protocols: `query`, `json_target`, `rest_xml`, `rest_json`
- Required top-level keys: `schemaVersion`, `service`, `protocol`, `flows`

## Manifest location and naming

- One file per service
- Directory: `manifests/guided`
- Naming convention: `<service>.guided.json`

## Required L1 flow semantics

Each service manifest must include at least one L1 flow with:

1. At least one execution step
2. At least one assertion
3. At least one cleanup step

## Expression model

Supported interpolation expressions:

- `{{inputs.<name>}}`
- `{{context.<name>}}`
- `{{captures.<name>}}`
- `{{rand8()}}`
- `{{timestamp()}}`

Unsupported expressions fail semantic lint.

## Example

```json
{
  "schemaVersion": "1.2",
  "service": "s3",
  "protocol": "rest_xml",
  "flows": [
    {
      "id": "l1-basic",
      "level": "L1",
      "steps": [
        {
          "id": "create",
          "title": "Create bucket",
          "operation": { "method": "PUT", "path": "/{{inputs.resource_name}}" },
          "assertions": [{ "kind": "status", "target": "status", "expected": "200" }]
        }
      ],
      "cleanup": [
        {
          "id": "delete",
          "title": "Delete bucket",
          "operation": { "method": "DELETE", "path": "/{{captures.resource_name}}" },
          "assertions": [{ "kind": "status", "target": "status", "expected": "200" }]
        }
      ]
    }
  ]
}
```

## Anti-patterns

- No cleanup in an L1 flow
- No assertions
- Multiple manifests for one service
- Unbounded or script-like expressions

## Validation commands

```bash
cargo test -p openstack-studio-ui guided_manifest::tests -- --nocapture
python3 scripts/studio_guided_coverage_gate.py
python3 scripts/studio_coverage_report.py
```

## Migration notes

- Minor upgrades (`1.x` -> `1.y`) can add fields and remain compatible.
- Major upgrades (`1.x` -> `2.x`) are breaking and require migration planning and release notes.
