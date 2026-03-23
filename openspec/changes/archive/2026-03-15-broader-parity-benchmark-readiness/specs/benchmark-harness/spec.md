## MODIFIED Requirements

### Requirement: Profile-based all-services benchmark coverage
The system SHALL support benchmark execution profiles that include broad all-services realistic coverage and deeper workloads for selected high-impact services. Each profile SHALL resolve to an intentional scenario set, and required broad coverage lanes SHALL maintain valid write and read performance scenarios for each supported service or an explicit machine-readable exclusion.

#### Scenario: All-services realistic profile covers enabled service set
- **WHEN** the all-services realistic benchmark profile is requested
- **THEN** the harness SHALL execute representative realistic benchmark scenarios for every configured benchmark service

#### Scenario: Every service includes write and read realistic scenarios
- **WHEN** required all-services realistic lanes run
- **THEN** each supported service SHALL have at least one measured write/mutate scenario and one measured read/query/list/describe scenario result, or an explicit machine-readable exclusion

#### Scenario: Deep profile targets high-impact service workloads
- **WHEN** the deep benchmark profile is requested
- **THEN** the harness SHALL execute additional workload scenarios for designated high-impact services with larger payloads and/or higher operation volume

#### Scenario: Broad lane scenario validity is enforced
- **WHEN** all-services benchmark lanes run
- **THEN** each supported service SHALL have valid realistic performance scenario coverage for required write/read roles or an explicit machine-readable exclusion reason

#### Scenario: Every configured profile resolves to scenarios or explicit diagnostics
- **WHEN** a benchmark profile such as `fair-high` or `fair-extreme` is requested
- **THEN** the harness SHALL either execute its resolved scenarios or emit an explicit machine-readable configuration/skip diagnostic instead of failing as an empty implicit profile

### Requirement: Reproducibility and fairness controls
The system SHALL provide benchmark execution controls that improve reproducibility and reduce biased comparisons. Scenario contracts SHALL keep warmup, setup, and measured operations semantically valid across both targets.

#### Scenario: Warmup is excluded from measured results
- **WHEN** a scenario defines warmup iterations
- **THEN** the harness SHALL execute warmup operations before measurement and SHALL exclude warmup timing from reported benchmark metrics

#### Scenario: Controlled iteration and concurrency settings are applied
- **WHEN** benchmark configuration specifies iteration count and concurrency level
- **THEN** the harness SHALL apply those settings identically for both targets during measured execution

#### Scenario: Warmup does not poison measured write operations
- **WHEN** a write scenario uses create-or-mutate operations that can fail on duplicate or already-consumed state
- **THEN** the harness SHALL ensure warmup and measured iterations remain semantically valid rather than converting the measured phase into guaranteed failure

#### Scenario: Target-specific identifiers are not hardcoded into shared scenarios
- **WHEN** a benchmark scenario requires resource identifiers such as queue URLs, topic ARNs, or service-generated names
- **THEN** the scenario SHALL derive those identifiers from setup outputs or run-context expansion rather than embedding LocalStack-specific endpoint shapes in the measured command

### Requirement: Machine-readable benchmark reporting
The system SHALL emit benchmark reports in a machine-readable format suitable for automation and trend analysis, and SHALL publish readable consolidated CI summaries across benchmark lanes. Reports SHALL distinguish product/runtime behavior gaps from harness limitations, configuration defects, and unsound scenario contracts.

#### Scenario: Benchmark report is written to disk
- **WHEN** a benchmark run completes
- **THEN** the harness SHALL write a JSON report containing run metadata, profile name, per-scenario metrics, and aggregate summary metrics

#### Scenario: Missing runtime evidence remains explicit
- **WHEN** a benchmark lane cannot collect runtime evidence such as memory RSS for one target
- **THEN** the report SHALL record the missing target explicitly rather than implying full observability coverage

#### Scenario: In-process runtime limitations are classified explicitly
- **WHEN** OpenStack runs in-process and container-based memory inspection is unavailable
- **THEN** the benchmark report SHALL classify the missing memory evidence as an explicit runtime-observability limitation rather than a silent omission or generic product failure

#### Scenario: Invalid scenario reason distinguishes contract defects
- **WHEN** a scenario is invalidated due to unsound setup, missing seeded state, or non-portable target-specific identifiers
- **THEN** the report SHALL record a machine-readable invalid reason that distinguishes scenario-contract defects from product behavior failures

### Requirement: Deep and diagnostic lanes remain visible without pretending required-lane completeness
The benchmark system SHALL report deep and non-required diagnostic lanes such as `hot-path-deep`, `fair-high`, and `fair-extreme` without applying required-lane completeness semantics that those lanes do not promise.

#### Scenario: Diagnostic lane reports partial role coverage without required-lane failure semantics
- **WHEN** a diagnostic or deep lane intentionally exercises only a subset of service roles
- **THEN** the report SHALL surface that partial coverage as diagnostic context without classifying the lane as missing required write/read coverage for every included service by default

#### Scenario: Non-required lane status is explicit
- **WHEN** a non-required lane completes, skips, or has unresolved scenario configuration
- **THEN** the report SHALL include an explicit lane status suitable for CI summaries and follow-up triage
