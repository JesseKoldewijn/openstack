## MODIFIED Requirements

### Requirement: Required CI lanes SHALL enforce benchmark regression thresholds
The CI system SHALL fail required benchmark lanes when measured performance regresses beyond configured thresholds versus comparison targets, evaluating openstack p95 latency ratios against each active comparison target (LocalStack and moto when present). Either ratio exceeding the threshold SHALL trigger a failure. The system SHALL document auth/token prerequisites needed for baseline discovery.

#### Scenario: Required lane fails on p95 latency regression breach against LocalStack
- **WHEN** the current lane `os_p95 / ls_p95` ratio exceeds the configured `--p95-threshold`
- **THEN** the benchmark gate SHALL fail the CI job with a message that includes current ratio, threshold, and lane name

#### Scenario: Required lane fails on p95 latency regression breach against moto
- **WHEN** moto is an active target and the current lane `os_p95 / moto_p95` ratio exceeds the configured `--p95-threshold`
- **THEN** the benchmark gate SHALL fail the CI job with a message that includes current ratio, threshold, and lane name

#### Scenario: Required lane fails on p99 latency regression breach
- **WHEN** the current lane p99 ratio regresses by more than the configured threshold relative to any active comparison target
- **THEN** the benchmark gate SHALL fail the CI job with a message that includes current value, threshold, and lane name

#### Scenario: Required lane fails on throughput regression breach
- **WHEN** the current lane throughput ratio regresses below the configured threshold relative to any active comparison target
- **THEN** the benchmark gate SHALL fail the CI job with a message that includes current value, threshold, and lane name

#### Scenario: Missing GitHub token prerequisite is explicit
- **WHEN** baseline discovery requires GitHub API access through CLI
- **THEN** workflow and gate diagnostics SHALL explicitly require `GH_TOKEN` (or equivalent) and provide remediation guidance when missing

#### Scenario: Gate skips moto ratio when moto is not active
- **WHEN** moto is not included in the active target set
- **THEN** the gate SHALL evaluate openstack only against LocalStack and SHALL NOT fail due to missing moto data

### Requirement: Consolidated benchmark summary SHALL be produced for CI readability
CI reporting SHALL produce one consolidated benchmark summary artifact that includes lane-level metrics, gate verdicts, and per-target comparison columns for the run.

#### Scenario: Consolidated summary includes all available fairness lanes
- **WHEN** benchmark lanes complete in a workflow run
- **THEN** CI SHALL emit a single markdown summary containing each available fairness lane with key metrics

#### Scenario: Consolidated summary includes gate verdicts
- **WHEN** required lane gate evaluation completes
- **THEN** the consolidated summary SHALL include pass/fail verdicts and threshold context for each required lane

#### Scenario: Markdown summary table includes per-target columns
- **WHEN** the gate generates a markdown summary table
- **THEN** the table SHALL include columns for OS p50, OS p95, OS p99, LS p95, Moto p95 (when active), OS/LS ratio, OS/Moto ratio (when active), OS RPS, LS RPS, Moto RPS (when active), and Status for each operation

#### Scenario: Markdown summary includes memory row for each active target
- **WHEN** the gate generates a markdown summary
- **THEN** the summary SHALL include memory usage (loaded RSS) for openstack and each active comparison target

#### Scenario: Consolidated summary includes explicit diagnostic-lane status
- **WHEN** non-required lanes are skipped, partially configured, or intentionally non-blocking
- **THEN** the consolidated summary SHALL include explicit status text describing whether the lane executed, skipped by policy, or failed due to scenario configuration

### Requirement: CI artifact download resilience
The CI PR comment job SHALL handle missing benchmark artifacts gracefully without failing the entire comment workflow.

#### Scenario: Missing benchmark artifact does not block PR comment
- **WHEN** the benchmark artifact download step in the PR comment job fails because the artifact was not uploaded (due to a skipped or failed benchmark job)
- **THEN** the download step SHALL use `continue-on-error: true` so the PR comment job continues and the downstream script handles missing files

### Requirement: Semgrep workflow removal
The Semgrep CI workflow SHALL be removed since CodeRabbit now provides equivalent analysis.

#### Scenario: Semgrep workflow file is deleted
- **WHEN** the change is applied
- **THEN** `.github/workflows/semgrep.yml` SHALL NOT exist in the repository

### Requirement: CI workflow moto integration
CI benchmark workflows SHALL include moto image management alongside the existing LocalStack and openstack image handling.

#### Scenario: CI benchmark jobs pull moto image in preflight
- **WHEN** a CI benchmark job starts
- **THEN** the workflow SHALL pull the moto Docker image as part of the preflight/setup steps

#### Scenario: CI benchmark jobs define moto image as environment variable
- **WHEN** the CI benchmark workflow is configured
- **THEN** the workflow SHALL define the moto Docker image reference as an environment variable for consistency across steps
