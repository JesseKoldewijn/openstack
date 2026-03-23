## Purpose
TBD

## Requirements

### Requirement: Service workload matrix SHALL define write and read operations for every supported service
The benchmark script SHALL include explicit write/mutate and read/query/list HTTP calls for every supported service. Operations are defined directly in the script as explicit HTTP requests with correct protocol headers, not in a separate machine-readable matrix file.

#### Scenario: All 24 services have defined operations
- **WHEN** the benchmark script is reviewed
- **THEN** every service in the supported list (s3, sqs, sns, dynamodb, iam, sts, kms, secretsmanager, ssm, acm, kinesis, firehose, cloudwatch, events, states, apigateway, ec2, route53, ses, ecr, opensearch, redshift, cloudformation, lambda) SHALL have a dedicated section with explicit HTTP benchmark calls

#### Scenario: Each service has at least one write and one read operation
- **WHEN** a service section is executed
- **THEN** the section SHALL include at least one write/mutate operation and one read/query/list/describe operation

#### Scenario: Operations use correct AWS protocol headers
- **WHEN** a benchmark HTTP call is made for a service
- **THEN** the call SHALL use the correct Content-Type header and X-Amz-Target prefix (for JSON protocol services) or Action parameter (for Query protocol services) or REST path (for REST protocol services)

### Requirement: Seed operations SHALL create prerequisite resources per service
The benchmark script SHALL create prerequisite resources (tables, buckets, streams, queues, topics, etc.) for each service before executing measured operations.

#### Scenario: Seed creates required resources
- **WHEN** a service benchmark section begins
- **THEN** the script SHALL create the resources needed for that service's write and read operations via direct HTTP calls

#### Scenario: Seed failure skips the service
- **WHEN** a seed operation fails for a service
- **THEN** the script SHALL skip the remaining operations for that service, log the failure, and record a skip entry with the service name and failure reason in the JSON output

#### Scenario: Seed operations are not measured
- **WHEN** prerequisite resources are created
- **THEN** the seed HTTP calls SHALL NOT be included in the measured benchmark metrics
