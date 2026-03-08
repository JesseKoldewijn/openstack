## 1. Harness Foundation

- [x] 1.1 Create parity harness module structure and configuration model for dual targets (openstack/localstack), execution profiles, and run metadata.
- [x] 1.2 Implement target lifecycle management (startup readiness checks, endpoint wiring, teardown) with deterministic retries/timeouts.
- [x] 1.3 Define scenario format/DSL that supports setup, request sequence, assertions, and cleanup for reuse across targets.

## 2. Differential Execution and Comparison

- [x] 2.1 Implement dual-target scenario runner that executes identical scenario flows against both targets and captures structured traces.
- [x] 2.2 Build protocol-aware normalizers/comparators for json, query/xml, rest-xml, and rest-json responses.
- [x] 2.3 Add side-effect verification hooks for follow-up state assertions and include results in parity decisions.

## 3. Reporting and Difference Governance

- [x] 3.1 Implement machine-readable parity report output (scenario pass/fail, diff classification, per-service parity score, evidence references).
- [x] 3.2 Add known-differences registry format with required metadata (scope, rationale, owner/reviewer, review/expiry) and matching logic.
- [x] 3.3 Enforce policy validation so malformed/expired known-difference entries fail parity checks.

## 4. Initial Scenario Coverage

- [x] 4.1 Port/create core parity scenarios for S3, SQS, DynamoDB, and STS covering create/read/delete and representative error paths.
- [x] 4.2 Add environment-variable-dependent compatibility scenarios (for example URL/host formatting and service enablement behavior) to parity coverage.
- [x] 4.3 Add deterministic fixtures/test data and cleanup guarantees to keep parity runs reproducible.

## 5. CI Integration and Rollout

- [x] 5.1 Add CI job that runs parity harness with pinned LocalStack image and publishes parity artifacts.
- [x] 5.2 Configure core profile as the initial required parity gate after stability validation; keep extended profile non-required.
- [x] 5.3 Document parity triage workflow (regression vs accepted difference), profile usage, and update process for known-difference entries.
