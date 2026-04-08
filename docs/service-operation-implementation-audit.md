# Service Operation Implementation Audit

Branch: `fix/service-operation-benchmark-coverage`
Date: 2026-04-08

## Goal
Determine whether any supposedly supported services are still missing operation implementations, and distinguish that from benchmark/test coverage gaps.

## Short Answer
Yes.

A quick repo-wide scan shows that many service providers still contain explicit `NotImplemented` / `NotImplementedException` fallback paths. That means the service crate exists and supports a meaningful subset of operations, but not necessarily the full API surface for that AWS service.

## Important Caveat
This audit used a **fast static scan** of provider dispatch code. It is highly useful for identifying gaps, but it is not a perfect source of truth for every provider because some services:
- route through helper functions or action enums
- derive operations indirectly from headers/path routing
- do not expose all operation names as simple string literals in a single `match`

So the audit below should be read as:
- **high-confidence signal for existence of missing operations**
- **good snapshot of implemented subsets for many services**
- **some services still need manual extraction of full implemented-op lists**

---

## High-confidence finding: explicit missing-operation fallbacks exist
The following services have explicit `NotImplemented` / `NotImplementedException` code paths in their providers:

- acm
- apigateway
- cloudformation
- cloudwatch
- dynamodb
- ec2
- ecr
- eventbridge
- firehose
- iam
- kinesis
- kms
- lambda
- opensearch
- redshift
- route53
- s3
- secretsmanager
- ses
- sns
- sqs
- ssm
- stepfunctions
- sts

That means “supported service” should currently be interpreted as:
- **supported subset implemented**, not
- **complete AWS operation surface implemented**.

---

## Snapshot: extracted implemented operation counts
These counts come from a fast static scan of direct string-literal dispatch branches.

### Higher-confidence extracted services
These providers expose operation names clearly enough that the scan is useful.

| Service | Extracted implemented ops | Explicit not-implemented fallback? |
|---|---:|---:|
| lambda | 26 | yes |
| cloudwatch | 20 | yes |
| ec2 | 11 | yes |
| cloudformation | 8 | yes |
| ecr | 8 | yes |
| eventbridge | 13 | yes |
| sns | 13 | yes |
| sqs | 14 | yes |
| stepfunctions | 9 | yes |
| ses | 4 | yes |
| opensearch | 4 | yes |
| redshift | 3 | yes |
| route53 | 6 | yes |

### Lower-confidence / manual-review-needed services
The quick parser undercounted these because of indirect dispatch structure.

| Service | Quick extracted count | Confidence |
|---|---:|---|
| acm | 0 | low |
| apigateway | 0 | low |
| dynamodb | 0 | low |
| firehose | 0 | low |
| iam | 0 | low |
| kinesis | 0 | low |
| kms | 0 | low |
| s3 | 0 | low |
| secretsmanager | 0 | low |
| ssm | 0 | low |
| sts | 0 | low |

These still have explicit not-implemented fallbacks, so the key conclusion remains valid even though the exact implemented-op count needs a manual pass.

---

## Benchmark coverage snapshot
Current benchmark coverage is broad, but many services still benchmark only a small subset of their likely implemented surface.

### Current benchmarked operations by service
- **acm**: `list_certificates`
- **apigateway**: `create_rest_api`, `get_rest_api`, `get_rest_apis`
- **cloudformation**: `create_stack`, `describe_stacks`, `list_stacks`
- **cloudwatch**: `put_metric_data`, `list_metrics`
- **dynamodb**: `put_item`, `get_item`, `query`, `scan`
- **ec2**: `run_instances`, `describe_instances`
- **ecr**: `create_repository`, `describe_repositories`, `list_images`, `batch_get_image`
- **eventbridge**: `put_rule`, `list_event_buses`, `list_rules`, `describe_rule`, `list_targets_by_rule`
- **firehose**: `put_record`, `put_record_batch`, `list_delivery_streams`
- **iam**: `create_user`, `get_user`, `list_users`
- **kinesis**: `put_record`, `describe_stream`, `list_streams`
- **kms**: `list_keys`
- **lambda**: `list_functions`, `get_function`, `invoke`, `update_function_configuration`, `update_function_code`, `delete_function`
- **opensearch**: `list_domain_names`
- **redshift**: `describe_clusters`
- **route53**: `list_hosted_zones`
- **s3**: put/get/head/list variants across size tiers
- **secretsmanager**: `create_secret`, `get_secret_value`, `put_secret_value`, `list_secrets`
- **ses**: `list_identities`
- **sns**: `list_topics`
- **sqs**: `list_queues`, `send_message`, `receive_message`
- **ssm**: `put_parameter`, `get_parameter`, `describe_parameters`
- **stepfunctions**: `list_state_machines`
- **sts**: `get_caller_identity`, `assume_role`

---

## Smoke/integration coverage snapshot
Current dedicated smoke coverage exists for:
- s3
- sqs
- dynamodb
- sns
- kms
- secretsmanager
- ssm
- kinesis
- opensearch
- cloudformation
- stepfunctions
- apigateway
- cloudwatch
- firehose
- route53
- iam
- sts
- lambda

### Missing dedicated smoke coverage (highest-value current gaps)
- acm
- ec2
- ecr
- eventbridge
- redshift
- ses

---

## Most likely implementation-risk services
These are the services I’d prioritize for a deeper manual implementation audit because they combine at least one of:
- explicit not-implemented fallback
- small benchmark surface
- weak or missing smoke coverage
- small extracted implemented-op surface

### Priority 1
- **acm**
- **redshift**
- **ses**
- **route53**
- **opensearch**
- **ec2**
- **eventbridge**
- **ecr**

### Priority 2
- **kms**
- **stepfunctions**
- **cloudformation**
- **cloudwatch**
- **apigateway**
- **ssm**

### Lower priority for implementation audit right now
These already have relatively broad practical coverage or stronger operational signal:
- s3
- dynamodb
- lambda
- sns
- sqs
- kinesis
- secretsmanager

---

## Recommendations

## 1. Treat implementation completeness and benchmark completeness as separate tracks
We should track four dimensions per service:
- implemented ops
- explicitly unsupported ops
- benchmarked ops
- smoke-tested ops

## 2. Do a manual operation extraction pass for indirect-dispatch providers
The quick scanner undercounts at least:
- acm
- apigateway
- dynamodb
- firehose
- iam
- kinesis
- kms
- s3
- secretsmanager
- ssm
- sts

These need a direct provider-by-provider manual inventory.

## 3. Continue benchmark expansion, but use implementation audit to avoid benchmarking non-existent ops
The current branch’s benchmark work is still valuable, but before adding many more operations we should confirm which ones are truly implemented on weaker services.

## 4. Add smoke coverage for the biggest untested services
Best next smoke additions:
- eventbridge
- ecr
- ses
- ec2
- acm
- redshift

---

## Proposed next follow-up
Create a **manual support matrix** file with one row per service:
- extracted implemented ops
- explicitly unsupported fallback present?
- benchmarked ops
- smoke tests present?
- next implementation gap to audit

That would become the source of truth for deciding whether a service needs:
- more implementation
- more benchmarks
- more smoke tests
- or all three.
