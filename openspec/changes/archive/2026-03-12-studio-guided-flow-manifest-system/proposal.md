## Why

Studio currently has foundational routing, metadata, and early interaction primitives, but it does not yet provide a truly guided, end-to-end interaction experience for every supported service. Without a manifest-driven guided flow system, service UX coverage will drift, become inconsistent, and require unsustainably high per-service bespoke UI maintenance.

Now is the right time to introduce a canonical guided-flow manifest contract because we already have the daemon/runtime, Studio API foundation, and initial guided flow concepts in place. Formalizing a protocol-aware manifest system now enables scalable guided coverage for all supported services while preserving consistency, testability, and long-term maintainability.

## What Changes

- Introduce a versioned Studio Guided Flow Manifest Contract (schema-first) that defines service-level guided flows, step operations, bindings, assertions, cleanup behavior, validation rules, and replay metadata.
- Add a protocol adapter layer (`query`, `json_target`, `rest_xml`, `rest_json`) that executes normalized manifest operations and isolates protocol-specific serialization/parsing logic from UI rendering.
- Implement a service-manifest registry and loader that discovers and validates guided manifests for all supported services at startup/build-time, with strict schema and semantic checks.
- Build generic Studio guided flow rendering/execution engine powered by manifests, including step timeline execution, error guidance, output capture, assertion verification, and cleanup orchestration.
- Add complete baseline guided flow manifests (L1 lifecycle flows) for all supported services in openstack.
- Add Studio coverage reporting and gating that enforces manifest existence and minimum guided flow quality criteria per service.
- Add robust test architecture across schema validation, adapter correctness, flow execution contracts, and end-to-end guided scenarios.
- Add authoring docs, flow style guide, and release-readiness criteria for continuous guided-flow maintenance as services evolve.

## Capabilities

### New Capabilities
- `studio-guided-flow-manifests`: Defines the canonical manifest schema and lifecycle requirements for guided flows across all supported services.
- `studio-protocol-adapters`: Defines protocol adapter behavior and guarantees for executing normalized guided operations.
- `studio-guided-flow-engine`: Defines guided flow runtime behavior, step orchestration, capture/binding semantics, assertions, and cleanup guarantees.
- `studio-guided-flow-coverage-governance`: Defines service coverage reporting, quality gates, and CI enforcement for all-service guided-flow completeness.

### Modified Capabilities
- `internal-api`: Extend requirements to expose manifest/capability metadata needed by the Studio guided flow engine and coverage reporting.
- `gateway-core`: Extend requirements for guided-flow request execution safety constraints and Studio route-level guardrails under larger all-service guided traffic.

## Impact

- Affected code:
  - `crates/studio-ui/*` (manifest model, rendering engine, execution state machine, authoring tools)
  - `crates/internal-api/*` (manifest discovery endpoints, coverage metadata endpoints)
  - `crates/gateway/*` (safety limits and route guardrails relevant to guided traffic)
  - new manifest assets directory (for service flow descriptors and schema files)
  - CI workflows and scripts for coverage validation and gating
- Affected APIs:
  - new/expanded Studio API endpoints for manifest catalog retrieval, flow definitions, and coverage metadata
  - stable schema versioning and compatibility expectations for Studio consumers
- Affected systems:
  - CI now enforces guided manifest coverage and validation across all supported services
  - release process includes guided-flow completeness and non-regression checks
- Dependencies/tooling:
  - JSON schema validation tooling, manifest linting, and coverage report generation
  - expanded E2E suites exercising guided flows for representative protocol classes and per-service lifecycle baselines
