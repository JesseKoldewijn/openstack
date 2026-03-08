## MODIFIED Requirements

### Requirement: Workflow runtime baseline and reporting
The system SHALL capture and publish baseline and ongoing workflow performance metrics for pull request and main-branch CI workflows, including total duration and per-job duration distributions for CI, Docker, and cross-compile workflows.

#### Scenario: Baseline captured before optimization
- **WHEN** a workflow optimization initiative starts
- **THEN** the system records at least a median and p95 duration baseline for each targeted workflow and its highest-cost jobs

#### Scenario: Post-change runtime comparison is available
- **WHEN** workflow changes are proposed or merged
- **THEN** the system provides a before/after comparison against the recorded baseline for affected workflows

### Requirement: Controlled matrix parallelism
The system SHALL support matrix-based parallel execution with configurable bounds to prevent runner saturation and excessive queue contention across CI and cross-compile workflows.

#### Scenario: Matrix jobs respect parallelism limits
- **WHEN** a workflow defines a matrix for multiple environments or feature sets
- **THEN** execution honors the configured parallelism cap and does not exceed it

#### Scenario: Matrix strategy remains deterministic
- **WHEN** the same commit is evaluated multiple times
- **THEN** the matrix configuration and resulting required checks remain stable and predictable

### Requirement: Selective execution with safety fallback
The system SHALL skip non-impacted optional jobs based on change scope and SHALL always retain a conservative fallback path that runs critical validation for CI, Docker, and cross-compile workflows.

#### Scenario: Non-impacted optional jobs are skipped
- **WHEN** changed files do not match the scope of an optional workflow job
- **THEN** that optional job is not executed for the run

#### Scenario: Critical validation still runs
- **WHEN** selective execution rules are applied
- **THEN** required security, build, and core test checks still execute for protected branches and pull requests

## ADDED Requirements

### Requirement: Docker and cross-compile optimization parity
The system SHALL apply workflow runtime optimization governance to Docker and cross-compile pipelines with the same rigor used for core CI pipelines.

#### Scenario: Optimization rollout includes Docker and cross-compile
- **WHEN** workflow optimization changes are implemented
- **THEN** Docker and cross-compile workflows are included in dependency, concurrency, and runtime validation plans

#### Scenario: Optimization success criteria are evaluated per workflow
- **WHEN** post-change metrics are reviewed
- **THEN** each workflow class (CI, Docker, cross-compile) is evaluated against its own baseline and regression thresholds
