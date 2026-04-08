# Service Coverage Matrix

Auto-generated working inventory for the `fix/service-operation-benchmark-coverage` branch.

## Method

- `implemented_ops`: approximate dispatch operations found in each `crates/services/*/src/provider.rs`
- `unit_tests`: count of `test_*` functions in each service crate test suite
- `perf_tests`: count of `perf_*` functions in each service crate test suite
- `smoke_tests`: integration smoke tests in `crates/tests/integration/tests/smoke_tests.rs`
- `bench_ops`: benchmark operations referenced in `tests/bench/bench_services.sh`

Notes:
- this is a working coverage map, not a formal proof of completeness
- a small ignore-list is applied for obvious non-dispatch literals in providers (for example Step Functions ASL node kinds)

## Summary

| service | implemented_ops | unit_tests | perf_tests | smoke_tests | bench_ops |
|---|---:|---:|---:|---:|---:|
| acm | 9 | 9 | 3 | 1 | 3 |
| apigateway | 12 | 13 | 3 | 1 | 4 |
| cloudformation | 7 | 13 | 3 | 1 | 4 |
| cloudwatch | 17 | 21 | 3 | 1 | 3 |
| dynamodb | 19 | 29 | 8 | 1 | 4 |
| ec2 | 11 | 12 | 3 | 1 | 3 |
| ecr | 8 | 14 | 5 | 2 | 4 |
| eventbridge | 14 | 16 | 2 | 1 | 5 |
| firehose | 6 | 7 | 3 | 1 | 4 |
| iam | 15 | 17 | 3 | 1 | 5 |
| kinesis | 17 | 18 | 4 | 1 | 3 |
| kms | 20 | 18 | 3 | 1 | 3 |
| lambda | 26 | 31 | 3 | 1 | 6 |
| opensearch | 6 | 8 | 3 | 1 | 4 |
| redshift | 5 | 9 | 3 | 1 | 3 |
| route53 | 8 | 8 | 3 | 1 | 4 |
| s3 | 27 | 34 | 6 | 8 | 4 |
| secretsmanager | 8 | 8 | 3 | 1 | 5 |
| ses | 7 | 9 | 3 | 1 | 4 |
| sns | 9 | 22 | 3 | 1 | 3 |
| sqs | 14 | 32 | 3 | 1 | 5 |
| ssm | 7 | 8 | 3 | 1 | 4 |
| stepfunctions | 9 | 12 | 3 | 1 | 3 |
| sts | 5 | 8 | 5 | 1 | 4 |

## Detailed per service

### acm

- implemented_ops (9): `AddTagsToCertificate`, `DeleteCertificate`, `DescribeCertificate`, `ExportCertificate`, `ImportCertificate`, `ListCertificates`, `ListTagsForCertificate`, `RemoveTagsFromCertificate`, `RequestCertificate`
- unit_tests: 9
- perf_tests: 3
- smoke_tests (1): `smoke_acm_certificate_lifecycle_with_latency_guardrail`
- bench_ops (3): `describe_certificate`, `list_certificates`, `request_certificate`

### apigateway

- implemented_ops (12): `CreateDeployment`, `CreateResource`, `CreateRestApi`, `DeleteRestApi`, `GetDeployments`, `GetMethod`, `GetResources`, `GetRestApi`, `GetRestApis`, `GetStages`, `PutIntegration`, `PutMethod`
- unit_tests: 13
- perf_tests: 3
- smoke_tests (1): `smoke_apigateway_lifecycle_with_latency_guardrail`
- bench_ops (4): `create_resource`, `create_rest_api`, `get_rest_api`, `get_rest_apis`

### cloudformation

- implemented_ops (7): `CreateStack`, `DeleteStack`, `DescribeStackResources`, `DescribeStacks`, `GetTemplate`, `ListStacks`, `UpdateStack`
- unit_tests: 13
- perf_tests: 3
- smoke_tests (1): `smoke_cloudformation_stack_lifecycle_with_latency_guardrail`
- bench_ops (4): `create_stack`, `describe_stacks`, `get_template`, `list_stacks`

### cloudwatch

- implemented_ops (17): `CreateLogGroup`, `CreateLogStream`, `DeleteAlarms`, `DeleteLogGroup`, `DescribeAlarms`, `DescribeLogGroups`, `DescribeLogStreams`, `FilterLogEvents`, `GetLogEvents`, `GetMetricData`, `GetMetricStatistics`, `ListMetrics`, `PutLogEvents`, `PutMetricAlarm`, `PutMetricData`, `PutRetentionPolicy`, `SetAlarmState`
- unit_tests: 21
- perf_tests: 3
- smoke_tests (1): `smoke_cloudwatch_metrics_with_latency_guardrail`
- bench_ops (3): `get_metric_statistics`, `list_metrics`, `put_metric_data`

### dynamodb

- implemented_ops (19): `BatchGetItem`, `BatchWriteItem`, `CreateTable`, `DeleteItem`, `DeleteTable`, `DescribeStream`, `DescribeTable`, `GetItem`, `GetRecords`, `GetShardIterator`, `ListStreams`, `ListTables`, `PutItem`, `Query`, `Scan`, `TransactGetItems`, `TransactWriteItems`, `UpdateItem`, `UpdateTable`
- unit_tests: 29
- perf_tests: 8
- smoke_tests (1): `smoke_dynamodb_table_lifecycle`
- bench_ops (4): `get_item`, `put_item`, `query`, `scan`

### ec2

- implemented_ops (11): `AuthorizeSecurityGroupIngress`, `CreateSecurityGroup`, `CreateSubnet`, `CreateVpc`, `DeleteVpc`, `DescribeInstances`, `DescribeSecurityGroups`, `DescribeSubnets`, `DescribeVpcs`, `RunInstances`, `TerminateInstances`
- unit_tests: 12
- perf_tests: 3
- smoke_tests (1): `smoke_ec2_instance_lifecycle_with_latency_guardrail`
- bench_ops (3): `create_vpc`, `describe_instances`, `run_instances`

### ecr

- implemented_ops (8): `BatchDeleteImage`, `BatchGetImage`, `CreateRepository`, `DeleteRepository`, `DescribeImages`, `DescribeRepositories`, `ListImages`, `PutImage`
- unit_tests: 14
- perf_tests: 5
- smoke_tests (2): `smoke_ecr_repository_lifecycle_with_latency_guardrail`, `smoke_secretsmanager_lifecycle`
- bench_ops (4): `batch_get_image`, `create_repository`, `describe_repositories`, `list_images`

### eventbridge

- implemented_ops (14): `CreateEventBus`, `DeleteEventBus`, `DeleteRule`, `DescribeEventBus`, `DescribeRule`, `DisableRule`, `EnableRule`, `ListEventBuses`, `ListRules`, `ListTargetsByRule`, `PutEvents`, `PutRule`, `PutTargets`, `RemoveTargets`
- unit_tests: 16
- perf_tests: 2
- smoke_tests (1): `smoke_eventbridge_rule_lifecycle_with_latency_guardrail`
- bench_ops (5): `describe_rule`, `list_event_buses`, `list_rules`, `list_targets_by_rule`, `put_rule`

### firehose

- implemented_ops (6): `CreateDeliveryStream`, `DeleteDeliveryStream`, `DescribeDeliveryStream`, `ListDeliveryStreams`, `PutRecord`, `PutRecordBatch`
- unit_tests: 7
- perf_tests: 3
- smoke_tests (1): `smoke_firehose_stream_lifecycle_with_latency_guardrail`
- bench_ops (4): `describe_delivery_stream`, `list_delivery_streams`, `put_record`, `put_record_batch`

### iam

- implemented_ops (15): `AddUserToGroup`, `AssumeRole`, `AttachRolePolicy`, `AttachUserPolicy`, `CreateGroup`, `CreatePolicy`, `CreateRole`, `CreateUser`, `DeleteRole`, `DeleteUser`, `GetPolicy`, `GetRole`, `GetUser`, `ListUsers`, `PutRolePolicy`
- unit_tests: 17
- perf_tests: 3
- smoke_tests (1): `smoke_iam_and_sts_query_with_latency_guardrail`
- bench_ops (5): `create_role`, `create_user`, `get_role`, `get_user`, `list_users`

### kinesis

- implemented_ops (17): `AddTagsToStream`, `CreateStream`, `DecreaseStreamRetentionPeriod`, `DeleteStream`, `DescribeStream`, `DescribeStreamSummary`, `GetRecords`, `GetShardIterator`, `IncreaseStreamRetentionPeriod`, `ListShards`, `ListStreams`, `ListTagsForStream`, `MergeShards`, `PutRecord`, `PutRecords`, `RemoveTagsFromStream`, `SplitShard`
- unit_tests: 18
- perf_tests: 4
- smoke_tests (1): `smoke_kinesis_stream_lifecycle`
- bench_ops (3): `describe_stream`, `list_streams`, `put_record`

### kms

- implemented_ops (20): `CancelKeyDeletion`, `CreateAlias`, `CreateKey`, `Decrypt`, `DeleteAlias`, `DescribeKey`, `DisableKey`, `EnableKey`, `Encrypt`, `GenerateDataKey`, `GenerateDataKeyWithoutPlaintext`, `GenerateRandom`, `ListAliases`, `ListKeys`, `ListResourceTags`, `ScheduleKeyDeletion`, `Sign`, `TagResource`, `UntagResource`, `Verify`
- unit_tests: 18
- perf_tests: 3
- smoke_tests (1): `smoke_kms_key_lifecycle`
- bench_ops (3): `create_key`, `describe_key`, `list_keys`

### lambda

- implemented_ops (26): `AddPermission`, `CreateAlias`, `CreateEventSourceMapping`, `CreateFunction`, `DeleteAlias`, `DeleteEventSourceMapping`, `DeleteFunction`, `GetAlias`, `GetEventSourceMapping`, `GetFunction`, `GetFunctionConfiguration`, `GetLayerVersion`, `GetPolicy`, `Invoke`, `ListAliases`, `ListEventSourceMappings`, `ListFunctions`, `ListLayerVersions`, `ListLayers`, `PublishLayerVersion`, `PublishVersion`, `RemovePermission`, `UpdateAlias`, `UpdateEventSourceMapping`, `UpdateFunctionCode`, `UpdateFunctionConfiguration`
- unit_tests: 31
- perf_tests: 3
- smoke_tests (1): `smoke_lambda_invoke_path_with_latency_guardrail`
- bench_ops (6): `delete_function`, `get_function`, `invoke`, `list_functions`, `update_function_code`, `update_function_configuration`

### opensearch

- implemented_ops (6): `CreateDomain`, `DeleteDomain`, `DescribeDomain`, `DescribeDomainConfig`, `ListDomainNames`, `UpdateDomainConfig`
- unit_tests: 8
- perf_tests: 3
- smoke_tests (1): `smoke_opensearch_domain_lifecycle_with_latency_guardrail`
- bench_ops (4): `create_domain`, `describe_domain`, `list_domain_names`, `update_domain_config`

### redshift

- implemented_ops (5): `CreateCluster`, `DeleteCluster`, `DescribeClusters`, `ModifyCluster`, `RebootCluster`
- unit_tests: 9
- perf_tests: 3
- smoke_tests (1): `smoke_redshift_cluster_lifecycle_with_latency_guardrail`
- bench_ops (3): `create_cluster`, `describe_clusters`, `reboot_cluster`

### route53

- implemented_ops (8): `ChangeResourceRecordSets`, `CreateHostedZone`, `DELETE`, `DeleteHostedZone`, `GetChange`, `GetHostedZone`, `ListHostedZones`, `ListResourceRecordSets`
- unit_tests: 8
- perf_tests: 3
- smoke_tests (1): `smoke_route53_hosted_zone_lifecycle_with_latency_guardrail`
- bench_ops (4): `change_resource_record_sets`, `create_hosted_zone`, `list_hosted_zones`, `list_resource_record_sets`

### s3

- implemented_ops (27): `AbortMultipartUpload`, `CompleteMultipartUpload`, `CopyObject`, `CreateBucket`, `CreateMultipartUpload`, `DeleteBucketPolicy`, `DeleteObject`, `DeleteObjects`, `GetBucketAcl`, `GetBucketLocation`, `GetBucketNotificationConfiguration`, `GetBucketPolicy`, `GetBucketVersioning`, `GetObjectAcl`, `HeadBucket`, `HeadObject`, `ListBuckets`, `ListMultipartUploads`, `ListObjectVersions`, `ListObjects`, `ListObjectsV2`, `PutBucketAcl`, `PutBucketNotificationConfiguration`, `PutBucketPolicy`, `PutBucketVersioning`, `PutObject`, `PutObjectAcl`
- unit_tests: 34
- perf_tests: 6
- smoke_tests (8): `smoke_s3_bucket_lifecycle`, `smoke_s3_concurrent_put_same_key`, `smoke_s3_copy_object_cross_bucket`, `smoke_s3_large_object_streaming`, `smoke_s3_large_put_no_deadlock`, `smoke_s3_multi_size_upload_round_trip`, `smoke_s3_multipart_upload`, `smoke_s3_persistence_round_trip`
- bench_ops (4): `get_object_${_s3_tier}`, `head_object_${_s3_tier}`, `list_objects_v2_${_s3_tier}`, `put_object_${_s3_tier}`

### secretsmanager

- implemented_ops (8): `CreateSecret`, `DeleteSecret`, `DescribeSecret`, `GetSecretValue`, `ListSecrets`, `PutSecretValue`, `RestoreSecret`, `UpdateSecret`
- unit_tests: 8
- perf_tests: 3
- smoke_tests (1): `smoke_secretsmanager_lifecycle`
- bench_ops (5): `create_secret`, `get_secret_value`, `list_secrets`, `put_secret_value`, `update_secret`

### ses

- implemented_ops (7): `DeleteIdentity`, `GetIdentityVerificationAttributes`, `ListIdentities`, `SendEmail`, `SendRawEmail`, `VerifyDomainIdentity`, `VerifyEmailIdentity`
- unit_tests: 9
- perf_tests: 3
- smoke_tests (1): `smoke_ses_identity_and_send_email_with_latency_guardrail`
- bench_ops (4): `list_identities`, `send_email`, `send_raw_email`, `verify_email_identity`

### sns

- implemented_ops (9): `FilterPolicy`, `GetSubscriptionAttributes`, `GetTopicAttributes`, `ListSubscriptions`, `ListSubscriptionsByTopic`, `ListTopics`, `Publish`, `SetSubscriptionAttributes`, `SetTopicAttributes`
- unit_tests: 22
- perf_tests: 3
- smoke_tests (1): `smoke_sns_topic_lifecycle`
- bench_ops (3): `get_topic_attributes`, `list_topics`, `publish`

### sqs

- implemented_ops (14): `ChangeMessageVisibility`, `ChangeMessageVisibilityBatch`, `CreateQueue`, `DeleteMessage`, `DeleteMessageBatch`, `DeleteQueue`, `GetQueueAttributes`, `GetQueueUrl`, `ListQueues`, `PurgeQueue`, `ReceiveMessage`, `SendMessage`, `SendMessageBatch`, `SetQueueAttributes`
- unit_tests: 32
- perf_tests: 3
- smoke_tests (1): `smoke_sqs_queue_lifecycle`
- bench_ops (5): `delete_message`, `get_queue_attributes`, `list_queues`, `receive_message`, `send_message`

### ssm

- implemented_ops (7): `DeleteParameter`, `DeleteParameters`, `DescribeParameters`, `GetParameter`, `GetParameters`, `GetParametersByPath`, `PutParameter`
- unit_tests: 8
- perf_tests: 3
- smoke_tests (1): `smoke_ssm_parameter_lifecycle`
- bench_ops (4): `describe_parameters`, `get_parameter`, `get_parameters_by_path`, `put_parameter`

### stepfunctions

- implemented_ops (9): `CreateStateMachine`, `DeleteStateMachine`, `DescribeExecution`, `DescribeStateMachine`, `ListExecutions`, `ListStateMachines`, `StartExecution`, `StopExecution`, `UpdateStateMachine`
- unit_tests: 12
- perf_tests: 3
- smoke_tests (1): `smoke_stepfunctions_lifecycle_with_latency_guardrail`
- bench_ops (3): `create_state_machine`, `list_state_machines`, `start_execution`

### sts

- implemented_ops (5): `AssumeRole`, `DecodeAuthorizationMessage`, `GetAccessKeyInfo`, `GetCallerIdentity`, `GetSessionToken`
- unit_tests: 8
- perf_tests: 5
- smoke_tests (1): `smoke_iam_and_sts_query_with_latency_guardrail`
- bench_ops (4): `assume_role`, `get_access_key_info`, `get_caller_identity`, `get_session_token`

## Immediate completion priorities

Services still missing dedicated perf coverage, with especially thin benchmark breadth:

