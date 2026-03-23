## Why

The current parity and benchmark harnesses still depend heavily on spawned AWS CLI processes, which adds client-side overhead, hides raw protocol behavior behind CLI formatting, and makes our coverage story weaker than the README implies for the 24 services we publicly list as supported. We need a native HTTP-driven validation model now so parity remains anchored to LocalStack's actual responses, benchmark numbers reflect backend behavior instead of process startup cost, and unsupported or still-divergent services can be surfaced explicitly as follow-up work instead of being masked by thin probe coverage.

## What Changes

- Replace AWS CLI execution in the parity harness with a native Rust HTTP transport that sends equivalent protocol-correct requests to both openstack and LocalStack and compares structured HTTP outcomes instead of CLI stdout/stderr.
- Replace benchmark execution drivers with a native HTTP workload path as the canonical benchmark transport so latency and throughput measurements reflect backend behavior rather than per-operation CLI process overhead.
- Preserve and expand parity fidelity by comparing status codes, protocol-meaningful headers, normalized payload bodies, and error structures for the same request sequence sent to both targets.
- Use the current parity surface as the migration baseline for all 24 README-listed services, treating today's all-services smoke scenarios and any existing passing service behavior as the starting compatibility contract.
- Define explicit follow-up handling for services or scenarios that still do not respond appropriately under native HTTP parity, including machine-readable exclusions, accepted differences where justified, and implementation follow-up tasks rather than silent omission.
- Update CI, documentation, and local workflow guidance so parity and benchmark lanes no longer require AWS CLI availability for core operation and clearly report per-service native HTTP coverage maturity.

## Capabilities

### New Capabilities
- None.

### Modified Capabilities
- `parity-harness`: parity execution and reporting requirements change from CLI-oriented traces to native HTTP request/response comparison while preserving dual-target equivalence and accepted-difference governance.
- `benchmark-harness`: benchmark execution requirements change so broad and deep benchmark lanes run through native HTTP drivers and report validity using protocol-native workloads across all README-listed services.
- `benchmark-signal-quality`: benchmark validity diagnostics must account for native HTTP workload support, follow-up exclusions, and the distinction between valid backend measurements and unsupported scenarios.
- `compatibility-layer`: CI-visible compatibility verification must continue to prove LocalStack-aligned behavior after the transport shift, with unsupported native parity gaps surfaced explicitly instead of being hidden behind CLI dependencies.

## Impact

- Affected code: `crates/tests/integration/src/parity.rs`, `crates/tests/integration/src/benchmark.rs`, shared test harness utilities, scenario definitions under `tests/parity/` and `tests/benchmark/`, CI workflow definitions, and harness documentation.
- Affected systems: parity CI lanes, benchmark CI lanes, report generation, known-difference governance, and developer local workflows.
- Dependencies/tooling: remove AWS CLI as a core execution dependency for parity and benchmark lanes; continue using Docker or managed endpoints where already required for target runtime orchestration.
- Risk surface: request translation correctness across all four supported protocol families, parity normalization drift, temporary service gaps during migration, and the need to preserve existing response-equivalence expectations for all 24 services listed in `README.md`.
