## Why

openstack claims LocalStack drop-in compatibility, but current tests are mostly single-target smoke checks against openstack alone. We need a differential parity harness that runs the same scenarios against both openstack and LocalStack so compatibility regressions are detected early and measured continuously.

## What Changes

- Add a parity harness test suite that executes identical request scenarios against two targets (openstack and LocalStack) and compares outcomes.
- Define canonical comparison rules for status codes, protocol-specific payloads, selected headers, and observable side effects.
- Add a known-differences registry so intentional/accepted deviations are explicit, reviewed, and versioned.
- Produce machine-readable parity reports (pass/fail, diff categories, per-service parity score) for local runs and CI.
- Integrate parity runs into CI in a controlled profile (core services first, expandable coverage over time).

## Capabilities

### New Capabilities
- `parity-harness`: Differential testing framework that drives identical scenarios against openstack and LocalStack, normalizes nondeterministic fields, compares protocol-aware responses/state effects, and emits actionable parity reports.

### Modified Capabilities
- `compatibility-layer`: Add a formal compatibility verification contract requiring parity checks in CI, documented accepted-difference policy, and regression visibility across supported services.

## Impact

- **Test architecture**: Introduces dual-target execution, comparison/normalization logic, and structured parity fixtures.
- **CI/runtime**: Adds LocalStack container dependency for parity jobs and increases CI execution time; requires scoped rollout and profile-based runs.
- **Quality signal**: Creates measurable compatibility baselines and prevents silent behavior drift.
- **Developer workflow**: Adds parity reports and triage paths (real regression vs accepted difference) to routine change validation.
