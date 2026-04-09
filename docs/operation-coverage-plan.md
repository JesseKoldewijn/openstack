# Operation Benchmark & Test Coverage Plan

## Goal
Expand benchmark and integration coverage so each implemented service is exercised by more than a single "happy-path read" operation where applicable, with emphasis on high-value CRUD/state-transition operations.

Branch: `fix/service-operation-benchmark-coverage`

## Current Coverage Snapshot

### Benchmark coverage by service
Current benchmark coverage is broad across services, but shallow for many of them.

- **1 op only**
  - acm: `list_certificates`
  - apigateway: `get_rest_apis`
  - cloudformation: `list_stacks`
  - cloudwatch: `list_metrics`
  - ec2: `describe_instances`
  - kms: `list_keys`
  - opensearch: `list_domain_names`
  - redshift: `describe_clusters`
  - route53: `list_hosted_zones`
  - ses: `list_identities`
  - ssm: `describe_parameters`
  - stepfunctions: `list_state_machines`

- **2 ops**
  - sts: `assume_role`, `get_caller_identity`

- **3 ops**
  - firehose: `list_delivery_streams`, `put_record`, `put_record_batch`
  - iam: `create_user`, `get_user`, `list_users`
  - kinesis: `describe_stream`, `list_streams`, `put_record`
  - sns: `get_topic_attributes`, `list_topics`, `publish`

- **4+ ops**
  - dynamodb, ecr, eventbridge, lambda, s3, secretsmanager, sqs

### Integration/smoke coverage
Smoke coverage is much healthier than benchmark coverage and includes lifecycle/state-transition checks for many services:

- Strong: `s3`, `sqs`, `dynamodb`, `sns`, `kms`, `secretsmanager`, `ssm`, `kinesis`, `opensearch`, `cloudformation`, `stepfunctions`, `apigateway`, `cloudwatch`, `firehose`, `route53`, `iam+sts`, `lambda`
- Weak or absent as dedicated smoke coverage: `acm`, `ec2`, `ecr`, `eventbridge`, `redshift`, `ses`

## Key Observation
The benchmark suite currently favors:
- list/read-only operations for many services
- a few write operations only for select services
- deep S3 coverage, but relatively shallow non-S3 coverage

This means we do not yet benchmark many of the operations that are most likely to reveal regressions in:
- state mutation latency
- serialization cost
- validation overhead
- hot-path locking behavior
- resource creation/deletion code paths

## Proposed Prioritization

## Phase 1 — Fill obvious benchmark gaps in weak services
Target services with only 1 benchmarked operation and add one write/create op plus one follow-up read/describe op where supported.

### ACM
Add:
- `import_certificate`
- `describe_certificate`

Why:
- measures write-path + lookup-path instead of list-only

### API Gateway
Add:
- `create_rest_api`
- `get_rest_api` or `delete_rest_api`

Why:
- current benchmark only covers listing; lifecycle latency is more interesting

### CloudFormation
Add:
- `create_stack`
- `describe_stacks`
- `delete_stack`

Why:
- tests state mutation path instead of list-only

### CloudWatch
Add:
- `put_metric_data`
- `get_metric_statistics` or `list_metrics` after write

Why:
- benchmarks ingestion path, not just enumeration

### EC2
Add:
- `run_instances`
- `terminate_instances`
- optionally `describe_instances` after seed

Why:
- current benchmark misses mutation-heavy EC2 paths

### KMS
Add:
- `create_key`
- `describe_key`
- `encrypt` / `decrypt` if implemented and stable

Why:
- list-only tells us very little about actual KMS workload behavior

### OpenSearch
Add:
- `create_domain`
- `describe_domain`
- `delete_domain`

Why:
- list-only is too shallow

### Redshift
Add:
- `create_cluster`
- `delete_cluster`
- keep `describe_clusters`

Why:
- mutation coverage is currently absent

### Route53
Add:
- `create_hosted_zone`
- `change_resource_record_sets`
- `list_resource_record_sets`

Why:
- Route53 write latency can differ materially from list performance

### SES
Add:
- `verify_email_identity` or equivalent supported identity write op
- `send_email`

Why:
- current list-only benchmark misses real usage patterns

### SSM
Add:
- `put_parameter`
- `get_parameter`
- `delete_parameter`

Why:
- describe-only misses the main SSM hot path

### Step Functions
Add:
- `create_state_machine`
- `start_execution`
- `describe_execution` or `delete_state_machine`

Why:
- lifecycle + execution path matters more than list-only

## Phase 2 — Strengthen mid-coverage services
Services that already have 2–3 benchmark ops but still miss important lifecycle/state transitions.

### STS
Keep as-is unless we implement more realistic assume-role variants.
Low priority.

### Firehose
Add:
- `describe_delivery_stream`
- `create_delivery_stream` if benchmarkable cheaply

### IAM
Add:
- `delete_user`
- optionally `create_role` / `get_role`

### Kinesis
Add:
- `create_stream`
- `delete_stream`

### SNS
Add:
- `create_topic`
- `delete_topic`
- possibly `subscribe`

## Phase 3 — Fill smoke/integration gaps
Add dedicated smoke/lifecycle tests for services that lack them entirely.

### Highest-priority new smoke tests
- `acm`
- `ec2`
- `ecr`
- `eventbridge`
- `redshift`
- `ses`

For each, prefer a small lifecycle or mutation-oriented test rather than list-only verification.

Examples:
- **acm**: import → list/describe
- **ec2**: run → describe → terminate
- **ecr**: create repository → put image (or seed equivalent) → list images
- **eventbridge**: put rule → list rules → delete rule
- **redshift**: create cluster → describe → delete
- **ses**: verify identity or send email path if stable

## Phase 4 — Benchmark/readiness hygiene
As we expand operations:
- keep benchmarks cheap enough for CI
- prefer seed once + benchmark the target operation many times
- avoid operations with long eventual-consistency semantics unless emulator guarantees immediate consistency
- add per-op p95 overrides only when repeated CI evidence shows runner noise rather than real regressions

## Recommended First Patch Set
This is the order I would implement:

1. **SSM**: add `put_parameter`, `get_parameter`, `delete_parameter`
2. **CloudWatch**: add `put_metric_data`
3. **API Gateway**: add `create_rest_api`
4. **CloudFormation**: add `create_stack`, `delete_stack`
5. **EC2**: add `run_instances`, `terminate_instances`
6. **Route53**: add `create_hosted_zone`, `list_resource_record_sets`
7. **Smoke tests** for `eventbridge`, `ecr`, `ses`

Reasoning:
- these are likely implemented already
- they add meaningful write-path coverage
- they should be benchmarkable without huge CI/runtime cost

## Acceptance Criteria
A good first milestone on this branch would be:
- every currently benchmarked service has at least **2 operations** covered unless the service genuinely only has one meaningful supported path
- services with full lifecycle semantics should have at least **one mutation op + one read/list op** benchmarked
- the services currently missing dedicated smoke coverage get at least one lifecycle smoke test each

## Notes
- Keep `jet-stack` strictly read-only; use only as reference.
- Prefer narrow additions over huge benchmark explosions.
- Preserve current strong S3 coverage; this branch is about improving breadth elsewhere.
