# OpenStack Studio

OpenStack Studio is a developer-focused UI and workflow layer exposed from the
main gateway.

## Routes

- `/_localstack/studio` - Studio shell entrypoint
- `/_localstack/studio-api/services` - service catalog metadata
- `/_localstack/studio-api/interactions/schema` - request/response schema metadata

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
```

## Troubleshooting

- If `status` shows `degraded`, check daemon logs and health endpoint reachability.
- If Studio API calls fail, verify `/_localstack/health` and service enablement.
