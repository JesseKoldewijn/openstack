## Context

OpenStack is moving from core functional parity toward full-service operational parity while targeting clear performance and resource advantages over LocalStack. Current benchmarking and parity workflows provide strong directional signal, but they do not yet enforce service-class-aware acceptance criteria, equivalent persistence-mode comparisons, or cross-service restart durability semantics. The architecture must support both parity and speed without treating them as competing goals.

## Goals / Non-Goals

**Goals:**
- Define service-class design targets so each supported service is evaluated against realistic and strict performance/resource expectations.
- Establish persistence parity as a first-class contract across services, including restart and recovery behavior.
- Ensure benchmark methodology isolates harness overhead from runtime overhead and compares equivalent operating modes.
- Enforce CI gates that combine functional parity and performance/resource non-regression.
- Preserve existing API compatibility and functional semantics while introducing stricter operational guarantees.

**Non-Goals:**
- Rewriting every service implementation in one step.
- Matching LocalStack internal architecture choices one-to-one when behavior can be matched with lower-overhead Rust-native design.
- Promising identical absolute timings across all host environments.

## Decisions

### Decision 1: Classify services by execution model and set class-specific targets
OpenStack SHALL classify supported services into at least three classes: in-proc stateful services, mixed orchestration services, and external-engine-adjacent services. Each class gets explicit latency, throughput, memory, and startup targets.

**Rationale:** A single global target obscures bottlenecks and creates false failures for services with fundamentally different runtime profiles.

**Alternatives considered:**
- Single global threshold for all services: rejected due to low diagnostic value.
- Per-service bespoke targets only: rejected due to maintainability and policy fragmentation.

### Decision 2: Persistence parity is contract-first and mode-equivalent
OpenStack SHALL define persistence behavior per service and compare parity only under equivalent modes (for example, memory-like mode vs memory-like mode, durable mode vs durable mode). Restart survival and recovery semantics become required parity scenarios.

**Rationale:** Comparing non-equivalent persistence modes produces misleading parity and performance conclusions.

**Alternatives considered:**
- Keep persistence out of parity scope: rejected because users rely on restart behavior.
- Enforce one universal storage backend for all services: rejected because service semantics differ.

### Decision 3: Benchmark lanes separate harness overhead from runtime cost
OpenStack SHALL maintain at least two benchmark lane families: (1) fairness-compatible lane and (2) low-overhead measurement lane. Gate decisions must include lane validity and report lane-specific diagnostics.

**Rationale:** Harness/process overhead can hide server-side wins and lead to incorrect optimization priorities.

**Alternatives considered:**
- One benchmark lane only: rejected due to attribution ambiguity.
- Benchmark only with synthetic microbenchmarks: rejected due to parity realism loss.

### Decision 4: Combined parity + performance gate policy
Required lanes SHALL fail if either parity validity drops below required thresholds or performance/resource budgets regress beyond allowed deltas. Gate output SHALL include deterministic failure categories.

**Rationale:** Performance-only gates can pass functionally broken behavior; parity-only gates can mask severe regressions.

**Alternatives considered:**
- Independent non-blocking gates: rejected because high-confidence release policy needs integrated pass/fail criteria.

### Decision 5: Incremental migration with compatibility-preserving milestones
Implementation SHALL proceed in waves, beginning with shared runtime paths and persistence infrastructure before broad service-specific tuning. Each wave includes parity checks, performance checks, and rollback-ready checkpoints.

**Rationale:** Full simultaneous change across all services creates unacceptable risk and low observability.

## Risks / Trade-offs

- [Risk] Tight gates increase short-term CI failures and slower merge velocity. -> Mitigation: phased enablement and class-based temporary baselines with explicit expiry.
- [Risk] Persistence parity requirements expose latent service-specific edge cases. -> Mitigation: add restart lifecycle scenario matrix and staged required-lane expansion.
- [Risk] Optimizations can unintentionally alter externally visible behavior. -> Mitigation: parity harness remains authoritative and blocks incompatible changes.
- [Risk] Equivalent-mode benchmarking adds operational complexity. -> Mitigation: codify lane configuration and mode metadata in reports and gate diagnostics.

## Migration Plan

1. Introduce service classification and reporting without strict enforcement.
2. Add persistence parity scenarios and collect baseline data across supported services.
3. Enable dual-lane benchmark reporting and classify lane validity.
4. Turn on combined gate logic in warning mode, then enforce for required lanes.
5. Expand required-lane coverage by service class until all supported services are covered.

## Open Questions

- Which service subsets are first-class required lanes for release blocking in phase one?
- What minimum persistence durability semantics are mandatory for each service family?
- How should startup and idle-resource budgets be normalized across CI environments?
