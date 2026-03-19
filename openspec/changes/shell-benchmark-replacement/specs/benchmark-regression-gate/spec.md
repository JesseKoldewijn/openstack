## MODIFIED Requirements

### Requirement: Benchmark gate SHALL evaluate within-run comparisons only
The benchmark gate SHALL determine pass/fail by comparing openstack metrics against LocalStack metrics from the same benchmark run. The gate SHALL NOT require, fetch, or reference any prior baseline runs. Gate criteria evaluate raw per-operation metrics without weighted averages.

#### Scenario: Gate evaluates per-operation latency ratios
- **WHEN** the gate evaluates a benchmark report
- **THEN** the gate SHALL compare each operation's openstack p95 latency against the same operation's LocalStack p95 latency and fail if the ratio exceeds the configured threshold (default: 1.5x)

#### Scenario: Gate evaluates memory budget
- **WHEN** the gate evaluates memory metrics
- **THEN** the gate SHALL fail if the openstack-to-LocalStack RSS ratio exceeds the configured budget (default: 0.20)

#### Scenario: Gate evaluates error rates
- **WHEN** the gate evaluates operation results
- **THEN** the gate SHALL fail if any openstack operation has a non-zero error count

#### Scenario: Gate does not require prior baselines
- **WHEN** the gate is invoked
- **THEN** the gate SHALL NOT attempt to download or reference previous benchmark run artifacts from GitHub Actions or any other source

### Requirement: Required lane result quality SHALL be validated before threshold checks
The benchmark gate SHALL validate that the benchmark report contains usable data before evaluating thresholds.

#### Scenario: Empty results fail the gate
- **WHEN** a benchmark report contains zero operation results
- **THEN** the gate SHALL fail with a data-quality error message

#### Scenario: All operations skipped fails the gate
- **WHEN** a benchmark report marks all operations as skipped
- **THEN** the gate SHALL fail with skip-reason context

### Requirement: Benchmark gate SHALL produce raw metric summaries
The gate SHALL produce markdown summaries showing raw per-operation metrics without weighted averages or cross-service aggregation.

#### Scenario: Summary shows per-operation metrics
- **WHEN** the gate produces its markdown output
- **THEN** the summary SHALL include a table of raw per-operation metrics for both targets with pass/fail status per operation

#### Scenario: Summary shows no weighted averages
- **WHEN** the gate produces its summary
- **THEN** the summary SHALL NOT include weighted averages, aggregate ratios, or cross-service combined metrics

#### Scenario: Summary shows overall verdict
- **WHEN** the gate completes
- **THEN** the summary SHALL include an overall PASS or FAIL verdict

## REMOVED Requirements

### Requirement: Baseline availability SHALL be mandatory for required lanes
**Reason**: The baseline-required approach is removed entirely. The new gate evaluates within-run comparisons (openstack vs LocalStack in the same run) and does not depend on prior run artifacts. This eliminates the failure mode where new branches or fresh repos cannot pass CI due to missing baselines.
**Migration**: No baseline seeding or recovery needed. The gate operates standalone.

### Requirement: Required CI lanes SHALL enforce benchmark regression thresholds
**Reason**: Regression thresholds comparing current-run-vs-prior-run are removed. The new gate compares openstack-vs-LocalStack within the same run. Historical trend tracking can be added as a separate concern later.
**Migration**: Gate criteria are now ratio-based (openstack p95 vs LocalStack p95) rather than delta-based (current vs baseline).

### Requirement: Non-required lanes SHALL remain diagnostic
**Reason**: The lane system (fair-low, fair-medium, fair-high, fair-extreme) is removed. Replaced by three profiles (smoke, standard, deep). There is no required vs non-required lane distinction — the CI workflow picks the appropriate profile for each trigger.
**Migration**: Use `--profile smoke` for lightweight PR checks, `--profile standard` for main-branch checks, `--profile deep` for scheduled runs.

### Requirement: Consolidated benchmark summary SHALL be produced for CI readability
**Reason**: The multi-lane consolidated summary is removed because the lane system no longer exists. Replaced by a single markdown summary per benchmark run showing raw per-operation metrics.
**Migration**: The gate script produces a single markdown summary per run. CI posts this as a PR comment.
