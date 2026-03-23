## 1. Shared Native HTTP Foundation

- [x] 1.1 Introduce shared native HTTP request, response, and trace types in the integration test harness crate for query/xml, json-target, rest-xml, and rest-json execution.
- [x] 1.2 Implement deterministic request metadata helpers for native parity and benchmark execution, including synthetic AWS-style routing headers and reusable protocol-specific content-type handling.
- [x] 1.3 Build a translator registry that maps existing scenario command vectors to canonical native HTTP requests and explicit unsupported-operation outcomes without AWS CLI fallback.
- [x] 1.4 Port existing capture and runtime-context substitution logic so native responses can populate scenario context values needed by follow-up steps.

## 2. Parity Harness Migration

- [x] 2.1 Replace AWS CLI step execution in `crates/tests/integration/src/parity.rs` with the shared native HTTP execution layer while preserving current profile and scenario selection behavior.
- [x] 2.2 Redesign parity trace comparison to evaluate structured HTTP evidence, including status codes, normalized response bodies, relevant protocol headers, transport failures, and error semantics.
- [x] 2.3 Preserve persistence and restart parity handling under the native transport path, including deterministic mismatch classification for restart and mode-related divergences.
- [x] 2.4 Extend parity report output to include native execution coverage status, per-service README baseline accounting, and explicit follow-up-required outcomes for unsupported or non-equivalent native scenarios.
- [x] 2.5 Reconcile known-difference governance with the new native mismatch taxonomy so accepted differences remain traceable after the transport migration.

## 3. Benchmark Harness Migration

- [x] 3.1 Replace benchmark AWS CLI execution paths with the shared native HTTP execution layer and remove CLI-based timing from measured benchmark operations.
- [x] 3.2 Migrate existing direct-http benchmark coverage into the shared translator model so benchmark and parity transports use the same canonical request definitions.
- [x] 3.3 Update benchmark validity accounting to classify unsupported native scenarios as exclusions or follow-up-required results instead of silent fallback behavior.
- [x] 3.4 Ensure all-services benchmark lanes account for every README-listed service and report missing native read/write workload support as explicit diagnostics.
- [x] 3.5 Remove or retire long-term benchmark execution-driver configuration once native HTTP is the canonical broad-lane transport.

## 4. README Service Baseline and Follow-up Governance

- [x] 4.1 Audit the 24 services listed in `README.md` against current parity scenarios and benchmark workloads to establish the migration baseline inventory.
- [x] 4.2 Validate native transport coverage for each README-listed service using the current parity surface as the starting contract and record which services already respond equivalently.
- [x] 4.3 For any README-listed service whose baseline native parity does not yet match LocalStack appropriately, add explicit machine-readable follow-up reporting and create concrete implementation follow-up tasks rather than omitting the gap.
- [x] 4.4 Review probe-style all-services scenarios and decide which should remain as baseline migration contracts versus which need upgraded lifecycle coverage in later follow-up work.

## 5. CI, Documentation, and Validation

- [x] 5.1 Update parity and benchmark documentation to remove AWS CLI as a core runtime requirement and document the native HTTP execution model and follow-up semantics.
- [x] 5.2 Update CI workflows and any harness preflight checks so parity and benchmark lanes no longer require AWS CLI availability for normal operation.
- [x] 5.3 Validate core and all-services parity profiles under the native transport path and confirm reports preserve LocalStack-referenced equivalence decisions.
- [x] 5.4 Validate representative benchmark lanes under the native transport path and confirm signal-quality diagnostics reflect native support gaps without hidden fallback.
- [x] 5.5 Capture final migration readiness evidence showing per-service README baseline coverage, explicit follow-up items, and any accepted differences that remain after the transport shift.
