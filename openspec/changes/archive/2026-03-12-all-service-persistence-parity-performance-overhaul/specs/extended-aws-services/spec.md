## MODIFIED Requirements

### Requirement: IAM identity and access management emulation
The system SHALL emulate the IAM API including user management (CreateUser, DeleteUser, ListUsers, GetUser), role management (CreateRole, DeleteRole, AssumeRole), policy management (CreatePolicy, AttachUserPolicy, AttachRolePolicy, PutRolePolicy), and group management. IAM resources SHALL be cross-region (global within an account). IAM emulation SHALL meet class-specific performance/resource envelopes while preserving parity semantics.

#### Scenario: Create user and attach policy
- **WHEN** a user is created and a managed policy is attached
- **THEN** `ListAttachedUserPolicies` SHALL return the attached policy

#### Scenario: IAM is global within account
- **WHEN** a user is created in `us-east-1`
- **THEN** `GetUser` in `eu-west-1` for the same account SHALL return the same user

### Requirement: STS security token service emulation
The system SHALL emulate the STS API including `GetCallerIdentity`, `AssumeRole`, `GetSessionToken`, and `GetAccessKeyInfo`. `GetCallerIdentity` SHALL return the account ID derived from the request's access key. STS emulation SHALL meet class-specific performance/resource envelopes.

#### Scenario: Get caller identity
- **WHEN** `GetCallerIdentity` is called with the default access key
- **THEN** the response SHALL contain account `000000000000`

#### Scenario: Assume role
- **WHEN** `AssumeRole` is called with a valid role ARN
- **THEN** the response SHALL contain temporary credentials with a session token

### Requirement: KMS key management emulation
The system SHALL emulate the KMS API including key management (CreateKey, DescribeKey, ListKeys, EnableKey, DisableKey, ScheduleKeyDeletion), cryptographic operations (Encrypt, Decrypt, GenerateDataKey, Sign, Verify), and alias management (CreateAlias, ListAliases). KMS emulation SHALL satisfy parity and class-based performance/resource budgets.

#### Scenario: Encrypt and decrypt
- **WHEN** plaintext is encrypted with a KMS key and the ciphertext is decrypted with the same key
- **THEN** the decrypted plaintext SHALL match the original

#### Scenario: Key rotation
- **WHEN** key rotation is enabled on a key
- **THEN** `GetKeyRotationStatus` SHALL report rotation as enabled

### Requirement: CloudFormation stack emulation
The system SHALL emulate the CloudFormation API including stack operations (CreateStack, DeleteStack, DescribeStacks, ListStacks, UpdateStack) and a template engine that creates/updates/deletes resources defined in CloudFormation templates. The system SHALL support at minimum: `AWS::S3::Bucket`, `AWS::SQS::Queue`, `AWS::SNS::Topic`, `AWS::DynamoDB::Table`, `AWS::Lambda::Function`, `AWS::IAM::Role`, `AWS::IAM::Policy`. CloudFormation emulation SHALL preserve parity semantics while meeting class-specific performance/resource envelopes.

#### Scenario: Create stack with S3 bucket
- **WHEN** `CreateStack` is called with a template defining an `AWS::S3::Bucket`
- **THEN** the bucket SHALL be created in the S3 service and `DescribeStacks` SHALL show status `CREATE_COMPLETE`

#### Scenario: Delete stack cleans up resources
- **WHEN** `DeleteStack` is called for a stack that created an S3 bucket and SQS queue
- **THEN** both the bucket and queue SHALL be deleted

#### Scenario: Stack outputs
- **WHEN** a template defines `Outputs` referencing resource attributes
- **THEN** `DescribeStacks` SHALL include the resolved output values

### Requirement: CloudWatch metrics emulation
The system SHALL emulate the CloudWatch API including metric operations (PutMetricData, GetMetricData, GetMetricStatistics, ListMetrics) and alarm operations (PutMetricAlarm, DescribeAlarms, DeleteAlarms, SetAlarmState). CloudWatch emulation SHALL satisfy parity requirements and class-based performance/resource envelopes.

#### Scenario: Put and get metric data
- **WHEN** metric data points are put for namespace `Custom/App` with metric name `RequestCount`
- **THEN** `GetMetricStatistics` SHALL return the aggregated values for the requested period

#### Scenario: Alarm state change
- **WHEN** an alarm is created and `SetAlarmState` is called with state `ALARM`
- **THEN** `DescribeAlarms` SHALL show the alarm in `ALARM` state

### Requirement: OpenSearch emulation
The system SHALL emulate the OpenSearch Service API including domain management (CreateDomain, DeleteDomain, DescribeDomain). The system SHALL optionally start an embedded OpenSearch-compatible engine or provide stub responses. OpenSearch emulation SHALL declare persistence mode behavior and SHALL meet class-based resource budgets.

#### Scenario: Create and describe domain
- **WHEN** `CreateDomain` is called with domain name `my-search`
- **THEN** `DescribeDomain` SHALL return the domain with an endpoint URL

#### Scenario: Durable mode domain metadata survives restart
- **WHEN** domain state is created in a declared durable mode and the runtime restarts
- **THEN** domain metadata visibility SHALL remain parity-consistent with declared durability semantics
