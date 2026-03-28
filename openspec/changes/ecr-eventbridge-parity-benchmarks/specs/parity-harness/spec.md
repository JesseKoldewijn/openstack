## ADDED Requirements

### Requirement: ECR all-services-smoke lifecycle scenario covers implemented CRUD operations
The all-services-smoke parity profile SHALL include an ECR lifecycle scenario that exercises the full implemented happy-path surface: repository creation, image push, image listing, batch image retrieval, and repository deletion. This scenario SHALL run against both openstack and LocalStack targets and SHALL verify response parity for each step.

#### Scenario: ECR lifecycle scenario runs in all-services-smoke profile
- **WHEN** the all-services-smoke parity profile is executed
- **THEN** the harness SHALL execute an ECR scenario with steps covering CreateRepository, PutImage, ListImages, BatchGetImage (by tag), and DeleteRepository in that order, with each step asserting success

#### Scenario: ECR repository is created in setup and removed in cleanup
- **WHEN** the ECR lifecycle scenario executes
- **THEN** a named repository SHALL be created in the setup phase using a run-scoped identifier, and deleted in the cleanup phase so no state leaks between runs

#### Scenario: ECR DescribeImages stub is retained as a separate scenario
- **WHEN** the all-services-smoke parity profile is executed
- **THEN** the existing `ecr-probe` scenario (calling describe-images and expecting failure) SHALL remain present alongside the lifecycle scenario, so stub parity remains explicitly verified

### Requirement: EventBridge all-services-smoke lifecycle scenario covers implemented CRUD operations
The all-services-smoke parity profile SHALL include an EventBridge lifecycle scenario that exercises the full implemented happy-path surface: event bus creation, rule creation, target attachment, rule state inspection and toggling, target removal, rule deletion, and bus deletion. This scenario SHALL run against both openstack and LocalStack targets and SHALL verify response parity for each step.

#### Scenario: EventBridge lifecycle scenario runs in all-services-smoke profile
- **WHEN** the all-services-smoke parity profile is executed
- **THEN** the harness SHALL execute an EventBridge scenario with steps covering CreateEventBus, PutRule (with schedule expression), DescribeRule, PutTargets, ListTargetsByRule, DisableRule, EnableRule, RemoveTargets, DeleteRule, and DeleteEventBus in that order, with each step asserting success

#### Scenario: EventBridge resources are created in setup and removed in cleanup
- **WHEN** the EventBridge lifecycle scenario executes
- **THEN** a named event bus SHALL be created in setup using a run-scoped identifier and deleted in cleanup, preventing state leakage between runs

#### Scenario: EventBridge PutEvents stub is retained as a separate scenario
- **WHEN** the all-services-smoke parity profile is executed
- **THEN** the existing `events-probe` scenario (calling put-events and expecting failure) SHALL remain present alongside the lifecycle scenario, so stub parity remains explicitly verified
