# Parity Harness

The parity harness compares behavior between openstack and LocalStack for the same scenarios.

## Profiles

- `core`: required CI profile intended for stable, high-signal compatibility coverage
- `extended`: non-required profile for broader coverage and iterative expansion
- `all-services-smoke`: full 24-service parity lane used for PRs targeting `main`
- `all-services-smoke-fast`: budget lane used for non-`main` PR targets (including `develop`)

## Run Locally

Requirements:

- `aws` CLI available
- Docker available (unless `PARITY_LOCALSTACK_ENDPOINT` is provided)

Run core profile:

```bash
cargo run -p openstack-integration-tests --bin parity_runner -- core
```

Run extended profile:

```bash
cargo run -p openstack-integration-tests --bin parity_runner -- extended
```

Run full all-services smoke profile:

```bash
cargo run -p openstack-integration-tests --bin parity_runner -- --profile all-services-smoke
```

Run fast all-services smoke profile:

```bash
cargo run -p openstack-integration-tests --bin parity_runner -- --profile all-services-smoke-fast
```

Optional overrides:

- `PARITY_OPENSTACK_ENDPOINT=http://127.0.0.1:4566`
- `PARITY_LOCALSTACK_ENDPOINT=http://127.0.0.1:4666`
- `PARITY_LOCALSTACK_IMAGE=localstack/localstack:3.7.2`

Reports are written to `target/parity-reports/*.json`.

## Triage Workflow

1. Run parity profile and inspect report mismatch entries.
2. Classify mismatch:
   - regression in openstack
   - upstream LocalStack behavior change
   - acceptable difference
3. For acceptable differences, add an explicit entry in `tests/parity/known_differences.json` with:
   - `id`, `service`, `scenario_id`, `step_id`, `path`
   - `rationale`, `owner`, `reviewer`
   - `review_date`, `expires_on` (YYYY-MM-DD)
4. Re-run profile to confirm the mismatch is marked as accepted-difference.

Malformed or expired known-difference entries fail parity runs by design.

## Scenario Files

- Base built-in scenarios are defined in `crates/tests/integration/src/parity.rs`.
- Optional profile-specific scenarios can be added in `tests/parity/scenarios/<profile>.json`.
- External scenarios support placeholders: `{{run_id}}`, `{{bucket}}`, `{{queue}}`, `{{table}}`.
- If an external scenario reuses a built-in `id`, it replaces the built-in scenario to avoid duplicate execution.
- `extended` includes all `core` scenarios plus any `profile: "extended"` scenarios.
