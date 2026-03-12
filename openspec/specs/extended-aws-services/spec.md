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

### Requirement: CloudWatch Logs emulation
The system SHALL emulate the CloudWatch Logs API including log group management (CreateLogGroup, DeleteLogGroup, DescribeLogGroups), log stream management (CreateLogStream, DescribeLogStreams), and log events (PutLogEvents, GetLogEvents, FilterLogEvents).

#### Scenario: Write and read log events
- **WHEN** log events are put to a log stream
- **THEN** `GetLogEvents` SHALL return the events in chronological order

### Requirement: EventBridge emulation
The system SHALL emulate the EventBridge API including event bus management (CreateEventBus, DeleteEventBus), rule management (PutRule, DeleteRule, ListRules, EnableRule, DisableRule), target management (PutTargets, RemoveTargets, ListTargetsByRule), and event publishing (PutEvents).

#### Scenario: Rule routes event to SQS target
- **WHEN** a rule with an event pattern matching `{"source": ["my.app"]}` has an SQS target, and an event with source `my.app` is put
- **THEN** the event SHALL be delivered to the SQS queue

### Requirement: Step Functions emulation
The system SHALL emulate the Step Functions API including state machine management (CreateStateMachine, DeleteStateMachine, DescribeStateMachine), execution management (StartExecution, DescribeExecution, ListExecutions, StopExecution), and ASL (Amazon States Language) interpretation for at minimum Task, Pass, Wait, Choice, Parallel, Map, Succeed, and Fail states.

#### Scenario: Execute a simple state machine
- **WHEN** a state machine with a Pass state is created and an execution is started with input `{"key": "value"}`
- **THEN** `DescribeExecution` SHALL eventually show status `SUCCEEDED` with the expected output

### Requirement: API Gateway emulation
The system SHALL emulate the API Gateway REST API including API management (CreateRestApi, DeleteRestApi), resource management (CreateResource, GetResources), method management (PutMethod, PutIntegration), deployment (CreateDeployment), and request routing to Lambda integrations.

#### Scenario: API Gateway invokes Lambda
- **WHEN** an API is created with a Lambda proxy integration and a request is sent to the deployed API URL
- **THEN** the Lambda function SHALL be invoked and its response SHALL be returned to the caller

### Requirement: EC2 emulation
The system SHALL emulate a subset of the EC2 API including VPC operations (CreateVpc, DescribeVpcs, DeleteVpc), subnet operations (CreateSubnet, DescribeSubnets), security group operations (CreateSecurityGroup, AuthorizeSecurityGroupIngress, DescribeSecurityGroups), and instance operations (RunInstances, DescribeInstances, TerminateInstances). EC2 emulation SHALL be metadata-only (no actual VMs).

#### Scenario: Create and describe VPC
- **WHEN** `CreateVpc` is called with CIDR `10.0.0.0/16`
- **THEN** `DescribeVpcs` SHALL return the VPC with the correct CIDR and a generated VPC ID

#### Scenario: Run instances (metadata only)
- **WHEN** `RunInstances` is called
- **THEN** `DescribeInstances` SHALL return instance(s) in `running` state with generated instance IDs, but no actual compute SHALL be provisioned

### Requirement: Route53 DNS emulation
The system SHALL emulate the Route53 API including hosted zone management (CreateHostedZone, DeleteHostedZone, ListHostedZones) and record set management (ChangeResourceRecordSets, ListResourceRecordSets).

#### Scenario: Create hosted zone and records
- **WHEN** a hosted zone is created for `example.com` and an A record is added
- **THEN** `ListResourceRecordSets` SHALL return the A record

### Requirement: SSM Parameter Store emulation
The system SHALL emulate the SSM Parameter Store API including PutParameter, GetParameter, GetParameters, GetParametersByPath, DeleteParameter, and DescribeParameters. The system SHALL support String, StringList, and SecureString parameter types.

#### Scenario: Put and get parameter
- **WHEN** a parameter `/app/config/db-host` is put with value `localhost`
- **THEN** `GetParameter` SHALL return the value `localhost`

#### Scenario: Get parameters by path
- **WHEN** parameters `/app/config/a` and `/app/config/b` exist
- **THEN** `GetParametersByPath` with path `/app/config/` SHALL return both parameters

### Requirement: Secrets Manager emulation
The system SHALL emulate the Secrets Manager API including secret management (CreateSecret, GetSecretValue, PutSecretValue, DeleteSecret, ListSecrets, DescribeSecret, UpdateSecret) and rotation scheduling.

#### Scenario: Create and retrieve secret
- **WHEN** a secret is created with name `my-secret` and value `s3cret`
- **THEN** `GetSecretValue` SHALL return `s3cret`

### Requirement: SES email emulation
The system SHALL emulate the SES API including identity management (VerifyEmailIdentity, ListIdentities) and email sending (SendEmail, SendRawEmail). Emails SHALL be stored in memory and accessible for test verification rather than actually sent.

#### Scenario: Send and verify email
- **WHEN** `SendEmail` is called with recipient `test@example.com`
- **THEN** the email SHALL be stored and retrievable through the internal API for test verification

### Requirement: ACM certificate emulation
The system SHALL emulate the ACM API including certificate management (RequestCertificate, DescribeCertificate, ListCertificates, DeleteCertificate). Certificates SHALL be automatically issued (no actual DNS/email validation).

#### Scenario: Request certificate
- **WHEN** `RequestCertificate` is called for domain `example.com`
- **THEN** `DescribeCertificate` SHALL show the certificate in `ISSUED` status

### Requirement: ECR container registry emulation
The system SHALL emulate the ECR API including repository management (CreateRepository, DeleteRepository, DescribeRepositories) and image management (PutImage, BatchGetImage, ListImages). Image data SHALL be stored in memory or on disk.

#### Scenario: Create repository and describe
- **WHEN** `CreateRepository` is called with name `my-app`
- **THEN** `DescribeRepositories` SHALL return the repository with a generated URI

### Requirement: Redshift emulation
The system SHALL emulate a subset of the Redshift API including cluster management (CreateCluster, DeleteCluster, DescribeClusters). Emulation SHALL be metadata-only.

#### Scenario: Create and describe cluster
- **WHEN** `CreateCluster` is called
- **THEN** `DescribeClusters` SHALL return the cluster in `available` status
