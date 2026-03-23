## Context

The current parity harness executes scenario steps by rendering AWS CLI-style commands and spawning `aws` for each step, then compares normalized stdout/stderr and success flags. The benchmark harness has started to move away from that model, but native HTTP execution is only partially implemented and still falls back to AWS CLI for most operations. At the same time, the repository already contains raw HTTP integration coverage patterns in `smoke_tests.rs` and reusable fake-SigV4 request helpers in `harness.rs`, which means the codebase already knows how to speak directly to the gateway for query, json-target, rest-xml, and rest-json services.

This change touches parity, benchmarks, CI, developer workflows, and coverage governance across all 24 services listed in `README.md`. The migration has to preserve the existing role of parity as a LocalStack comparison tool, use the current parity profiles and scenario sets as a baseline contract, and avoid hiding service gaps during the transport migration. Where services or operations still fail to respond appropriately under the native HTTP driver, the system needs explicit reporting and follow-up tracking rather than implicit exclusion.

Constraints:

- Parity must remain dual-target and LocalStack-referenced, not degrade into openstack-only smoke tests.
- Benchmark numbers must become less sensitive to CLI process startup and CLI serialization overhead.
- The current scenario corpus is authored in AWS CLI-shaped command vectors, so a big-bang rewrite to literal wire definitions would add substantial migration risk and authoring cost.
- The repository already has capability specs and CI lanes that depend on machine-readable parity and benchmark reports, accepted-difference governance, and service-level coverage accounting.

## Goals / Non-Goals

**Goals:**
- Introduce a shared native HTTP execution layer that can send equivalent protocol-correct requests to openstack and LocalStack without AWS CLI involvement.
- Preserve the existing scenario corpus initially by translating current command-style scenario steps into canonical HTTP requests rather than forcing an immediate scenario format rewrite.
- Upgrade parity comparison to structured HTTP response equivalence, including status, normalized headers when relevant, normalized bodies, and error semantics.
- Make native HTTP the canonical benchmark transport for broad and deep benchmark lanes so comparative metrics reflect backend behavior.
- Keep all 24 README-listed services in scope and use the current parity surface as the migration baseline, including explicit reporting of services that still need follow-up work.
- Preserve accepted-difference handling, persistence parity classification, and per-service report outputs.

**Non-Goals:**
- Rewriting all scenario files into a new literal HTTP schema in the same change.
- Expanding supported service count beyond the 24 services already listed in `README.md`.
- Solving every current service response divergence as part of the transport migration itself.
- Replacing every external-client compatibility script in the repository; this change focuses on parity and benchmark execution engines.

## Decisions

### 1. Introduce a shared protocol-aware native HTTP driver used by both parity and benchmarks

The implementation will add a shared execution module in the integration harness crate that accepts a scenario step plus runtime context and produces a canonical HTTP request for one of the supported protocol families. Both parity and benchmark runners will call the same request builder and transport layer rather than maintain separate per-tool implementations.

Rationale:
- The current benchmark direct-HTTP path proves the transport is viable but is too narrow and benchmark-specific.
- `smoke_tests.rs` already contains service-specific raw HTTP examples that can be consolidated into reusable translators.
- Sharing the execution layer keeps parity and benchmark semantics aligned and prevents drift between a "comparison path" and a "performance path".

Alternatives considered:
- Keep separate parity and benchmark drivers: simpler locally, but guarantees duplicated protocol logic and divergent behavior.
- Translate scenarios into raw shell `curl` calls: still external-process driven and weaker for structured comparison/metrics.

### 2. Preserve the existing scenario schema in the first migration phase and translate command vectors into HTTP requests

Scenario steps will continue to carry the current `command: Vec<String>` representation in the first phase. A translator layer will map supported command shapes into canonical request definitions, using the existing `protocol` field plus service/operation-specific translation logic.

Rationale:
- Current parity and benchmark scenarios already cover the 24 README-listed services and provide the baseline contract we want to preserve.
- Replacing transport without replacing the authoring model reduces migration risk and keeps the change incremental.
- Translators can be built from existing smoke-test request patterns, letting us validate behavior before considering a later schema cleanup.

Alternatives considered:
- Replace scenario steps immediately with literal HTTP definitions: cleaner end state, but too disruptive and would obscure whether failures are due to transport migration or scenario rewrite.
- Keep AWS CLI-shaped scenarios and keep some CLI fallback paths: undermines the goal of eliminating CLI overhead and weakens comparability.

### 3. Parity will compare structured HTTP traces, not success booleans or CLI stdout/stderr

The parity harness will evolve `StepTrace` into a richer HTTP-native record that captures request method/path, selected routing headers, response status, response headers of interest, raw response body, normalized body, transport error state, and semantic success classification. Mismatch comparison will operate on those structured traces.

Parity mismatch categories will include at least:
- request translation failure
- transport error mismatch
- status mismatch
- response header mismatch (for a curated subset of protocol-meaningful headers)
- response body mismatch
- error body mismatch
- persistence-mode mismatch / restart recovery mismatch
- follow-up-required unsupported-native-operation

Rationale:
- The user requirement is to keep verifying that openstack responds in the same way as LocalStack.
- HTTP-native comparison is closer to the actual compatibility surface than CLI-formatted stdout/stderr and can detect differences the CLI hides.
- Structured mismatch types preserve known-difference governance and enable explicit follow-up reporting.

Alternatives considered:
- Direct-HTTP transport with success-only parity checks: insufficient because many compatibility differences are in body/error shape rather than status alone.
- Reconstruct fake CLI-style output from HTTP responses and keep current comparison logic: adds indirection and preserves the wrong abstraction.

### 4. Native HTTP parity will use canonical synthetic AWS-like request metadata rather than full SDK signing

The driver will standardize on deterministic request headers sufficient for service routing and protocol handling, reusing the existing fake SigV4-style authorization pattern where needed and consistent protocol-specific headers such as `x-amz-target`, `content-type`, and path/host conventions. This keeps requests identical across openstack and LocalStack while avoiding a new signing dependency in the first phase.

Rationale:
- The repository already uses fake SigV4 helpers successfully in integration coverage.
- Real signing adds implementation complexity without necessarily increasing parity value if neither target verifies signatures strictly in the compared paths.
- Deterministic synthetic headers are easier to normalize and reason about.

Alternatives considered:
- Implement full SigV4 signing immediately: more realistic, but adds dependency and complexity before the transport migration is proven.
- Use unsigned requests wherever possible: too fragile for service routing and less representative across protocol families.

### 5. Use a service-operation translator registry with explicit unsupported outcomes

The shared native driver will be organized around a registry that maps a command signature to a translator capable of building an HTTP request and interpreting captures. Unsupported operations will not silently fall back to AWS CLI. Instead, they will produce explicit machine-readable unsupported results that parity and benchmark reporting can count and surface.

Rationale:
- The user explicitly wants gaps called out as follow-up actions if services do not yet respond appropriately.
- Explicit unsupported results make migration completeness measurable for all 24 README services.
- A translator registry gives a clear implementation checklist and avoids giant ad hoc match statements that are hard to audit.

Alternatives considered:
- Silent fallback to AWS CLI per unsupported operation: preserves coverage superficially but defeats the purpose of native transport migration.
- Hard-fail the entire lane on the first unsupported operation: too disruptive during phased migration and obscures aggregate coverage state.

### 6. Parity reports become the authoritative migration baseline and must record native HTTP coverage maturity per service

The existing all-services smoke parity surface will be treated as the baseline inventory of operations across the 24 README services. During and after migration, reports will record which service scenarios are fully native, which produce expected equivalence, which are accepted differences, and which require follow-up due to unsupported translation or incorrect responses.

Rationale:
- There are no checked-in parity report artifacts in the repository today, so the practical baseline is the current scenario corpus and whatever services already respond correctly under those scenarios.
- Service-level maturity fields let maintainers see whether the transport migration is actually complete for the README surface.
- This satisfies the requirement to mark currently non-equivalent behavior as follow-up rather than losing visibility.

Alternatives considered:
- Treat migration as complete once the driver exists, regardless of per-service outcomes: too weak for the public README support claim.
- Require immediate pass parity for every service before merging any transport change: unrealistic and likely to block incremental progress.

### 7. Benchmarks will remove execution-driver choice once native HTTP reaches required service coverage

The benchmark harness currently models execution driver as `aws-cli|direct-http`. This change will move benchmark execution to native HTTP as the canonical path, with any temporary migration flags treated as short-lived internal scaffolding. Broad and deep lanes must report exclusions explicitly where a native request translator or semantically valid workload is still missing.

Rationale:
- Benchmark signal quality depends on removing client process overhead from the measured path.
- Keeping AWS CLI as a long-term benchmark driver invites accidental regression to a noisier transport.
- Explicit exclusions are consistent with existing benchmark-signal-quality requirements.

Alternatives considered:
- Keep dual benchmark drivers permanently: useful for debugging, but confusing for required CI interpretation and contrary to the intended performance contract.

## Risks / Trade-offs

- [Translator correctness differs from AWS CLI behavior] -> Mitigation: derive translators from existing passing smoke-test wire patterns, validate each translator against both targets, and preserve current scenario IDs so regressions are easy to attribute.
- [Parity becomes stricter and exposes more differences than before] -> Mitigation: keep accepted-difference governance intact, add normalized protocol-aware comparisons, and classify unsupported/native-gaps distinctly from true compatibility regressions.
- [Some services in the 24-service README set do not yet have complete native request translation or parity-equivalent behavior] -> Mitigation: require explicit per-service follow-up reporting in parity/benchmark outputs and tasks rather than silent fallback.
- [Benchmark comparability drifts if request-building overhead differs materially by protocol/service] -> Mitigation: share one native driver, keep setup/workload symmetry across targets, and record scenario validity/exclusion metadata in reports.
- [Synthetic SigV4 headers may diverge from stricter future routing requirements] -> Mitigation: isolate signing/header generation in one module so the system can later upgrade to real signing without redesigning scenario definitions.
- [Migration touches many existing CI and local workflow assumptions] -> Mitigation: phase the rollout, update docs alongside code, and keep machine-readable reports backward-compatible where practical.

## Migration Plan

1. Introduce shared HTTP request/response trace types and a translator registry that can cover the existing core parity scenarios and the benchmark direct-HTTP operations already implemented.
2. Port benchmark execution first or in parallel behind internal scaffolding, removing CLI fallback for covered operations and surfacing explicit unsupported results for the remainder.
3. Port parity execution to the shared HTTP driver, upgrading comparison and reporting from CLI traces to HTTP-native structured traces.
4. Extend translators and normalization until all current all-services smoke scenarios for the 24 README-listed services execute natively and produce machine-readable service maturity outcomes.
5. Update CI/documentation to drop AWS CLI as a required dependency for parity and benchmark lanes.
6. Resolve or explicitly track any remaining service-specific response mismatches as accepted differences or follow-up implementation work.

Rollback strategy:
- Because this is test and harness infrastructure, rollback is primarily code-path reversion rather than data migration.
- During phased delivery, temporary internal feature switches may preserve the old path for debugging, but the final design target is native HTTP only for parity and benchmarks.

## Open Questions

- Should parity compare a curated subset of response headers or only those known to carry protocol semantics, to avoid noisy diffs from server/runtime metadata?
- Do we want to formalize a dedicated per-service native-coverage maturity field in parity and benchmark reports, or encode that status through exclusion/mismatch classes only?
- For services with today’s probe-style failure scenarios, should the first native baseline preserve those exact failing operations, or should some services be upgraded concurrently to more meaningful lifecycle parity scenarios?
- At what point should the command-vector scenario format be retired in favor of a more explicit protocol-native schema, if ever?
