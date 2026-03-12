# Studio Guided Flow Troubleshooting

## Symptoms and fixes

### 1) Flow definition endpoint returns empty `flows`

- Check manifest exists in `manifests/guided/<service>.guided.json`
- Verify `schemaVersion` matches supported `1.2`
- Verify JSON parses successfully

### 2) Manifest lint fails with unsupported expression

- Ensure only these sources are used:
  - `inputs.*`
  - `context.*`
  - `captures.*`
  - `rand8()` and `timestamp()`

### 3) Guided execution reports assertion failure

- Validate expected status code and payload shape in manifest assertions
- Check adapter protocol class matches service protocol
- Use replay from interaction history to inspect the exact request

### 4) Coverage gate fails in CI

- Run `python3 scripts/studio_guided_coverage_gate.py`
- Add missing manifest files for services listed under `missing_services`

### 5) Gateway rejects guided execution request

- Allowed methods for guided execution endpoints are `POST` only
- Payload must be within configured guided endpoint limit

## Useful commands

```bash
cargo test -p openstack-studio-ui -p openstack-internal-api -p openstack-gateway
cargo test -p openstack-integration-tests --test studio_guided_manifest_e2e_tests
python3 scripts/studio_guided_coverage_gate.py
python3 scripts/studio_coverage_report.py
```
