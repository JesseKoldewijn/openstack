# Service Performance Remediation Matrix

This matrix tracks performance remediation coverage for every supported service.

## Service Matrix

| Service | Baseline Operation | Primary Hotspot Hypothesis | Target Gain Band |
|---|---|---|---|
| acm | `list-certificates` | JSON serialization and dispatch overhead | p95 -10% to -20% |
| apigateway | `get-rest-apis` | REST routing + response shaping overhead | p95 -10% to -20% |
| cloudformation | `list-stacks` | Query/XML parse and response encode overhead | p95 -10% to -20% |
| cloudwatch | `list-metrics` | metric list serialization and filtering path | throughput +10% to +25% |
| dynamodb | `list-tables` / `get-item` | table metadata access and key path allocations | p95 -15% to -30% |
| ec2 | `describe-instances` | query parsing + large response serialization | throughput +10% to +20% |
| ecr | `describe-repositories` | state lookup and JSON encode overhead | p95 -10% to -20% |
| events | `list-rules` | event rule filtering and serialization | p95 -10% to -20% |
| firehose | `list-delivery-streams` | state traversal and response encoding | throughput +10% to +20% |
| iam | `list-users` | query/XML overhead and identity object cloning | p95 -10% to -20% |
| kinesis | `list-streams` / `put-record` | stream metadata lock contention and encode path | throughput +15% to +30% |
| kms | `list-keys` | JSON marshalling and key metadata path | p95 -10% to -20% |
| lambda | `list-functions` | function metadata cloning and response size | throughput +10% to +25% |
| opensearch | `list-domain-names` | domain state traversal and serialization | p95 -10% to -20% |
| redshift | `describe-clusters` | response build overhead | p95 -10% to -20% |
| route53 | `list-hosted-zones` | XML serialization and route data copy overhead | throughput +10% to +20% |
| s3 | `list-buckets` / `put-object` | request-body buffering and object metadata path | p95 -20% to -40% |
| secretsmanager | `list-secrets` | secret metadata filtering/serialization | p95 -10% to -20% |
| ses | `list-identities` | query/XML parse and list response shaping | p95 -10% to -20% |
| sns | `list-topics` / `publish` | topic metadata lock contention and query path | throughput +10% to +25% |
| sqs | `list-queues` / `send-message` | queue URL normalization and message path allocations | p95 -15% to -30% |
| ssm | `describe-parameters` | parameter list filtering/serialization | p95 -10% to -20% |
| states | `list-state-machines` | state-machine metadata clone/serialize path | throughput +10% to +25% |
| sts | `get-caller-identity` | query parse + auth context handling | p95 -10% to -20% |

## Platform-Loop Backlog (Cross-Service)

| Area | Candidate Bottleneck | Acceptance Target |
|---|---|---|
| Gateway | request context cloning/header map rebuilding | reduce per-request alloc count by >=20% |
| Protocol | repeated parse/serde conversions in query/json/rest paths | p95 parse stage latency -20% |
| Service framework | dispatch overhead and error-path formatting | p95 dispatch overhead -15% |
| State layer | lock granularity/contention and map access patterns | throughput +20% under fair-high |
| Benchmark harness | subprocess and scenario validity overhead | 100% required-lane interpretable results |

## Service-Loop Waves

1. Wave 1: `s3`, `sqs`, `dynamodb`, `cloudwatch`
2. Wave 2: `kinesis`, `lambda`, `sns`, `ssm`, `states`
3. Wave 3: remaining control-plane/list services

Each wave requires parity revalidation before promotion.
