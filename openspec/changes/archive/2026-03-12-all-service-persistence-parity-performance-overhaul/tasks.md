## 1. Service classification and policy foundation

- [x] 1.1 Define and codify service execution classes (`in-proc-stateful`, `mixed-orchestration`, `external-engine-adjacent`) for every supported service.
- [x] 1.2 Add class-aware performance/resource envelopes (latency, throughput, memory, startup) with lane-specific policy configuration.
- [x] 1.3 Expose class metadata in benchmark/parity artifacts and diagnostics.

## 2. Persistence parity architecture and contracts

- [x] 2.1 Define persistence mode taxonomy and equivalence rules used by parity and benchmark tooling.
- [x] 2.2 Implement durability-class declarations per service and surface them through diagnostics.
- [x] 2.3 Extend persistence lifecycle hooks and error classes to emit deterministic save/load/recovery diagnostics.
- [x] 2.4 Add startup behavior for required durable modes to fail fast on unrecoverable persisted state.

## 3. Cross-service persistence fidelity validation

- [x] 3.1 Add persistence lifecycle scenarios (create state, restart, recover, validate) for all supported services in parity harness profiles.
- [x] 3.2 Add mode-mismatch detection in parity workflows and mark runs non-interpretable when targets are non-equivalent.
- [x] 3.3 Add machine-readable persistence failure classes and evidence payloads in parity reports.

## 4. Benchmark methodology hardening

- [x] 4.1 Add explicit persistence-mode metadata and equivalence checks to benchmark run configuration.
- [x] 4.2 Ensure dual-lane benchmark execution separates harness-influenced and low-overhead measurement paths.
- [x] 4.3 Extend consolidated benchmark reporting with service class, mode context, and lane interpretability fields.

## 5. Regression gate upgrades

- [x] 5.1 Extend benchmark gate logic to enforce class-specific envelope checks in required lanes.
- [x] 5.2 Add deterministic gate failure categories for `mode_mismatch`, class-envelope breaches, and persistence-quality failures.
- [x] 5.3 Ensure required-lane gate output includes per-service class diagnostics with remediation guidance.

## 6. Shared runtime performance tracks

- [x] 6.1 Optimize gateway-core request-path overhead and allocation hotspots while preserving protocol parity semantics.
- [x] 6.2 Optimize service-framework startup and concurrent access paths to satisfy startup/concurrency budgets.
- [x] 6.3 Add observability for gateway/service-framework contention and lifecycle metrics used by required-lane gating.

## 7. Core and extended service wave execution

- [x] 7.1 Execute Wave A for core services (S3, SQS, SNS, DynamoDB family) with parity + performance/resource acceptance checks.
- [x] 7.2 Execute Wave B for extended services (IAM, STS, KMS, CloudFormation, CloudWatch family, EventBridge, Step Functions, API Gateway, EC2 metadata subset, Route53, SSM, Secrets Manager, SES, ACM, ECR, OpenSearch, Redshift).
- [x] 7.3 For each wave, validate declared persistence behavior and restart semantics under equivalent target modes.

## 8. CI rollout and governance

- [x] 8.1 Roll out class-aware and persistence-aware gates in warning mode, then enforce for required lanes.
- [x] 8.2 Establish baseline refresh and expiration policy for temporary thresholds during migration waves.
- [x] 8.3 Publish progress dashboard artifacts showing parity pass rate, performance envelope pass rate, and persistence fidelity status by service class.

## 9. Verification and completion

- [x] 9.1 Add/extend automated tests for service classification, persistence mode equivalence checks, and deterministic failure-class reporting.
- [x] 9.2 Validate one full passing path and one intentional failing path for required-lane gates with persistence-aware diagnostics.
- [x] 9.3 Produce final change validation report demonstrating all supported services meet parity requirements and class-based performance/resource expectations.
