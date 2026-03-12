## Why

OpenStack currently has strong functional momentum, but its performance profile and persistence behavior are not yet consistently aligned with LocalStack across all supported service families. We need a coordinated change that preserves parity semantics while making Rust-native execution materially faster and lighter in CPU, memory, startup, and steady-state throughput.

## What Changes

- Define a cross-service performance and resource strategy with explicit service-class targets (in-proc services, mixed services, external-engine-adjacent services).
- Upgrade persistence from primarily in-memory service state toward durable, parity-aware state behavior across all supported services, including restart survival, crash consistency expectations, and parity-compatible isolation boundaries.
- Establish benchmark lanes that separate harness overhead from server/runtime overhead and compare equivalent storage/runtime modes between OpenStack and LocalStack.
- Introduce service-specific parity/performance acceptance criteria for latency (p95/p99), throughput, memory (RSS/alloc pressure), binary/runtime startup, and persistence correctness.
- Add regression gates that fail on both functional parity drift and performance/resource regressions in required service lanes.
- Expand cross-service parity validation so persistence semantics are covered for all supported services, not only core-wave service subsets.

## Capabilities

### New Capabilities
- `all-service-performance-classes`: Classify supported services by execution model and enforce class-specific performance/resource targets and reporting.
- `cross-service-persistence-fidelity`: Define and validate durable state semantics (save/load/restart parity) for all supported services.

### Modified Capabilities
- `state-persistence`: Extend requirements from basic persistence availability to parity-grade durability, isolation, and restart behavior across service families.
- `core-aws-services`: Add non-regression requirements that core services must meet defined performance/resource envelopes while preserving functional parity.
- `extended-aws-services`: Add service-family performance/resource and persistence parity requirements for extended service coverage.
- `benchmark-harness`: Require dual-lane benchmarking (harness-influenced and low-overhead driver lanes) with equivalent mode comparisons.
- `benchmark-regression-gate`: Gate on service-class metrics and persistence-aware parity validity, not only aggregate benchmark ratios.
- `parity-harness`: Require persistence lifecycle parity scenarios (create state, restart, recover, re-validate behavior) for supported services.
- `gateway-core`: Add request-path overhead and allocation budget requirements tied to parity-safe optimizations.
- `service-framework`: Add startup/concurrency/resource-budget requirements for service lifecycle orchestration under full-service load.

## Impact

- Affected systems: gateway request pipeline, service framework lifecycle/state orchestration, service providers and stores, parity harness, benchmark harness, regression gate, CI workflows, and reporting.
- Affected APIs/behavior: persistence lifecycle behavior and restart semantics become explicitly testable and required for parity claims.
- Dependencies: expanded benchmark datasets/artifacts, persistence test fixtures, CI matrix updates for required service lanes, and stricter gate policies for performance/resource parity.
- Risk surface: tighter gates may initially increase failures until storage/runtime paths and harness signal quality are stabilized across all service classes.
