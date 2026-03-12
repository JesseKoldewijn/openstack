## 1. Schema and Manifest Foundations

- [ ] 1.1 Add versioned Studio guided-flow JSON schema file and schema loading utilities.
- [ ] 1.2 Implement manifest parser and structural validator with actionable error reporting.
- [ ] 1.3 Implement semantic lint checks (required L1 flow structure, assertion presence, cleanup requirements, expression safety rules).
- [ ] 1.4 Define manifest storage layout and naming conventions for one-manifest-per-service enforcement.
- [ ] 1.5 Add manifest schema compatibility guardrails for major/minor version behavior.

## 2. Canonical Operation and Expression Runtime

- [ ] 2.1 Define canonical normalized operation model shared across all protocol adapters.
- [ ] 2.2 Implement expression interpolation engine supporting approved sources (`inputs`, `context`, `captures`, built-ins).
- [ ] 2.3 Implement expression validator rejecting unsupported syntax/sources and unsafe constructs.
- [ ] 2.4 Implement capture extraction model and binding propagation primitives.
- [ ] 2.5 Add unit tests for canonical operation serialization semantics and expression resolution edge cases.

## 3. Protocol Adapter Layer

- [ ] 3.1 Implement query adapter execution path (form/query serialization, response extraction helpers, error normalization).
- [ ] 3.2 Implement json_target adapter execution path (target headers, JSON body handling, capture/assertion extraction).
- [ ] 3.3 Implement rest_xml adapter execution path (path/query handling, XML capture/assertion extraction).
- [ ] 3.4 Implement rest_json adapter execution path (REST+JSON handling and normalized error mapping).
- [ ] 3.5 Add adapter conformance test harness with golden fixtures for each protocol class.
- [ ] 3.6 Add regression tests for protocol-specific failure and retryability classification behavior.

## 4. Guided Flow Engine Runtime

- [ ] 4.1 Implement deterministic guided flow state machine (pending/running/success/failure/canceled states).
- [ ] 4.2 Implement step orchestration with timeout, retry envelope, and terminal outcome recording.
- [ ] 4.3 Implement assertion evaluator supporting status/header/body/json-path/xml-path/resource checks.
- [ ] 4.4 Implement cleanup orchestration policy for success/failure paths with cleanup result reporting.
- [ ] 4.5 Implement interaction history emission and replay payload generation for guided executions.
- [ ] 4.6 Add runtime engine integration tests covering multi-step binding and failure-with-cleanup scenarios.

## 5. Studio UI Integration for Manifest-Driven Flows

- [ ] 5.1 Add manifest-backed service catalog model showing guided maturity and flow availability per service.
- [ ] 5.2 Implement generic guided flow renderer from manifest definitions (inputs, step timeline, assertions panel, cleanup panel).
- [ ] 5.3 Implement user input form generation and validation from manifest input schema.
- [ ] 5.4 Implement guided execution UX states (running/failed/succeeded), including error guidance rendering.
- [ ] 5.5 Implement replay UX integration from guided history entries into request/flow state.
- [ ] 5.6 Add component/integration tests for guided renderer behavior across protocol classes.

## 6. Internal API and Runtime Metadata

- [ ] 6.1 Add Studio API endpoint for guided manifest catalog index (`/_localstack/studio-api/flows/catalog`).
- [ ] 6.2 Add Studio API endpoint for service flow definition retrieval (`/_localstack/studio-api/flows/{service}`).
- [ ] 6.3 Add Studio API endpoint for guided coverage metrics (`/_localstack/studio-api/flows/coverage`).
- [ ] 6.4 Extend `/_localstack/info` metadata with manifest schema version and guided coverage summary references.
- [ ] 6.5 Add API contract tests for new flow metadata endpoints and metadata shape guarantees.

## 7. Gateway Guardrails and Safety Constraints

- [ ] 7.1 Implement Studio guided execution method allow-list enforcement in gateway/internal route handling.
- [ ] 7.2 Implement payload bound checks for guided execution endpoints with explicit rejection semantics.
- [ ] 7.3 Add security regression tests for disallowed methods and oversized payload rejection behavior.
- [ ] 7.4 Validate no regression in AWS route dispatch behavior under expanded Studio guided traffic.

## 8. All-Service Manifest Authoring (L1 Baseline)

- [ ] 8.1 Generate/author L1 guided manifest for each supported service with create/use/verify/cleanup semantics.
- [ ] 8.2 Add per-service manifest review checklist ensuring naming, assertions, and cleanup quality conventions.
- [ ] 8.3 Add representative flow fixtures for every protocol class and selected edge-case services.
- [ ] 8.4 Validate all manifests against schema and semantic linting in local and CI workflows.

## 9. E2E, Coverage Governance, and CI Enforcement

- [ ] 9.1 Add E2E suite validating manifest-driven guided execution for representative services in each protocol class.
- [ ] 9.2 Add governance script that cross-checks service registry against manifest inventory.
- [ ] 9.3 Add CI gate to fail when any supported service lacks at least one valid L1 guided flow.
- [ ] 9.4 Add CI coverage report artifact summarizing guided level and flow quality per service.
- [ ] 9.5 Add CI gate for manifest schema validation and semantic lint compliance.

## 10. Documentation, Authoring Tooling, and Release Readiness

- [ ] 10.1 Write guided manifest authoring guide (schema reference, examples, anti-patterns, migration notes).
- [ ] 10.2 Add manifest contribution template and validation command docs for contributors.
- [ ] 10.3 Add troubleshooting guide for guided flow execution failures and adapter-level diagnostics.
- [ ] 10.4 Add release readiness checklist covering manifest completeness, conformance tests, and non-regression suites.
- [ ] 10.5 Add change-management notes describing schema evolution policy and compatibility expectations.
