## ADDED Requirements

### Requirement: Account ID from access key
The system SHALL derive an AWS account ID from the access key ID in the request's `Authorization` header. The default account ID SHALL be `000000000000`. Different access key IDs SHALL map to different account IDs to support multi-account testing.

#### Scenario: Default account ID
- **WHEN** a request uses the default/test access key
- **THEN** the request context account ID SHALL be `000000000000`

#### Scenario: Custom access key maps to different account
- **WHEN** a request uses access key `AKIAIOSFODNN7EXAMPLE` configured to map to account `111111111111`
- **THEN** the request context account ID SHALL be `111111111111`

#### Scenario: Unknown access key uses derivation
- **WHEN** a request uses an unregistered access key
- **THEN** the system SHALL derive a deterministic account ID from the access key value

### Requirement: Multi-region request routing
The system SHALL route requests to the correct regional state store based on the region extracted from the request's `Authorization` header credential scope. Each `(account, region)` combination SHALL have independent service state.

#### Scenario: Create resources in different regions
- **WHEN** an SQS queue `q1` is created in `us-east-1` and queue `q2` in `eu-west-1`
- **THEN** `ListQueues` in `us-east-1` SHALL return only `q1` and `ListQueues` in `eu-west-1` SHALL return only `q2`

### Requirement: Default region fallback
The system SHALL use `us-east-1` as the default region when no region can be determined from the request.

#### Scenario: No region in request
- **WHEN** a request has no `Authorization` header and no region-indicating headers
- **THEN** the system SHALL use `us-east-1` as the region

### Requirement: ARN generation with correct account and region
All generated ARNs SHALL contain the correct account ID and region from the request context. ARN format: `arn:aws:<service>:<region>:<account-id>:<resource>`.

#### Scenario: ARN contains request account
- **WHEN** account `111111111111` creates an SQS queue `my-queue` in `us-west-2`
- **THEN** the queue ARN SHALL be `arn:aws:sqs:us-west-2:111111111111:my-queue`

### Requirement: Cross-account resource access
The system SHALL support cross-account resource access patterns where one account references resources in another account by ARN (e.g., SNS subscription from one account to an SQS queue in another).

#### Scenario: Cross-account SNS to SQS
- **WHEN** account `111` subscribes account `222`'s SQS queue to account `111`'s SNS topic
- **THEN** messages published to the topic SHALL be delivered to account `222`'s queue
