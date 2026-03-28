## ADDED Requirements

### Requirement: ECR benchmark section SHALL include seeding, write operations, and read operations
The ECR benchmark section in `bench_services.sh` SHALL seed a repository and a tagged image before measured operations, SHALL benchmark at least one write operation (CreateRepository with dynamic unique names), and SHALL benchmark at least two read operations (DescribeRepositories, ListImages). BatchGetImage SHALL be benchmarked as a read operation using the seeded image tag.

#### Scenario: ECR seed creates repository and pushes an image
- **WHEN** the ECR benchmark section begins
- **THEN** the script SHALL call CreateRepository for a named seed repository and PutImage to push an image with a fixed tag (`bench-img-<pid>`) before any measured operations begin

#### Scenario: ECR write benchmark exercises CreateRepository with unique names
- **WHEN** ECR write benchmarks are measured
- **THEN** the script SHALL call CreateRepository with per-iteration unique names (via `{i}` substitution) using `bench_dynamic_targets` so each request creates a new resource

#### Scenario: ECR read benchmarks cover describe, list, and batch-get operations
- **WHEN** ECR read benchmarks are measured
- **THEN** the script SHALL call DescribeRepositories (no parameters), ListImages (using the seed repository name), and BatchGetImage (using the seed repository name and the fixed seed image tag) as separate measured operations

#### Scenario: ECR seed failure skips all ECR benchmark operations
- **WHEN** the ECR seed step fails for any active target
- **THEN** the script SHALL skip the ECR write and read benchmarks and emit a skip entry in the JSON report

### Requirement: EventBridge benchmark section SHALL include seeding, write operations, and read operations
The EventBridge benchmark section in `bench_services.sh` SHALL seed an event bus, a rule, and at least one target before measured operations, SHALL benchmark at least one write operation (PutRule with dynamic unique names), and SHALL benchmark at least two read operations (ListEventBuses, ListRules, DescribeRule, ListTargetsByRule). All operations SHALL use the correct `AWSEvents.*` X-Amz-Target header.

#### Scenario: EventBridge seed creates an event bus, a rule, and targets
- **WHEN** the EventBridge benchmark section begins
- **THEN** the script SHALL call CreateEventBus for a named seed bus, PutRule for a named seed rule on that bus with a schedule expression, and PutTargets to attach at least one target to the seed rule before any measured operations begin

#### Scenario: EventBridge write benchmark exercises PutRule with unique names
- **WHEN** EventBridge write benchmarks are measured
- **THEN** the script SHALL call PutRule with per-iteration unique names (via `{i}` substitution) using `bench_dynamic_targets` so each request creates a new rule

#### Scenario: EventBridge read benchmarks cover list and describe operations
- **WHEN** EventBridge read benchmarks are measured
- **THEN** the script SHALL call ListEventBuses, ListRules (filtering by the seed bus name), DescribeRule (by the seed rule name), and ListTargetsByRule (by the seed rule name) as separate measured operations

#### Scenario: EventBridge seed failure skips all EventBridge benchmark operations
- **WHEN** the EventBridge seed step fails for any active target
- **THEN** the script SHALL skip the EventBridge write and read benchmarks and emit a skip entry in the JSON report
