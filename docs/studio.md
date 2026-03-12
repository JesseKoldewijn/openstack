# OpenStack Studio

OpenStack Studio is a developer-focused UI and workflow layer exposed from the
main gateway.

## Routes

- `/_localstack/studio` - Studio shell entrypoint
- `/_localstack/studio-api/services` - service catalog metadata
- `/_localstack/studio-api/interactions/schema` - request/response schema metadata
- `/_localstack/studio-api/flows/catalog` - guided flow catalog
- `/_localstack/studio-api/flows/{service}` - per-service guided manifest definition
- `/_localstack/studio-api/flows/coverage` - guided coverage metrics

## Daemon workflow

```bash
openstack start --daemon
openstack status
openstack logs --follow
openstack stop
```

## Test commands

```bash
cargo test -p openstack-studio-ui
cargo test -p openstack-integration-tests --test studio_e2e_tests
cargo test -p openstack-integration-tests --test studio_guided_manifest_e2e_tests
python3 scripts/studio_guided_coverage_gate.py
```

## Guided manifest docs

- `docs/studio-guided-manifest-authoring.md`
- `docs/studio-guided-manifest-contribution-template.md`
- `docs/studio-guided-flow-troubleshooting.md`
- `docs/studio-guided-flow-change-management.md`

## Troubleshooting

- If `status` shows `degraded`, check daemon logs and health endpoint reachability.
- If Studio API calls fail, verify `/_localstack/health` and service enablement.
