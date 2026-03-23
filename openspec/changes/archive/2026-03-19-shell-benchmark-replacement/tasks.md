## 1. Scaffold the new benchmark script

- [x] 1.1 Create `tests/bench/bench_services.sh` with shebang, `set -euo pipefail`, and argument parsing (`--profile`, `--services`, `--binary`, `--output`, `--requests`, `--concurrency`)
- [x] 1.2 Implement HTTP bench tool detection (check for `oha` in PATH, fall back to `hey`, fail with install instructions if neither found)
- [x] 1.3 Implement the `bench()` helper function that wraps oha/hey invocation, extracts p50/p95/p99/throughput/errors from output, and appends results to the JSON report via `jq`
- [x] 1.4 Implement profile resolution: map `smoke`/`standard`/`deep` to service lists, request counts, and concurrency values; apply `--services`/`--requests`/`--concurrency` overrides
- [x] 1.5 Implement helper functions: `log()`, `log_section()`, `get_docker_mem_kb()`, `update_results()` (jq wrapper for building the results JSON)

## 2. Implement target startup and memory collection

- [x] 2.1 Implement Docker mode: start openstack and LocalStack containers with configurable CPU/memory limits, wait for health endpoints, record container IDs
- [x] 2.2 Implement binary mode (`--binary`): start openstack binary as a background process, start LocalStack in Docker, wait for both health endpoints
- [x] 2.3 Implement idle memory snapshot collection (docker stats for containers, /proc VmRSS or ps for binary) after targets are healthy
- [x] 2.4 Implement post-load memory snapshot collection after all benchmark operations complete
- [x] 2.5 Implement cleanup trap to stop/remove containers and kill processes on exit

## 3. Implement per-service benchmark sections (core 8)

- [x] 3.1 S3 (REST-XML): seed bucket, bench PutObject, GetObject, HeadObject, ListObjectsV2
- [x] 3.2 DynamoDB (JSON): seed table, bench PutItem, GetItem, Query, Scan
- [x] 3.3 SNS (Query-XML): seed topic, bench Publish, GetTopicAttributes, ListTopics
- [x] 3.4 IAM (Query-XML): seed user, bench CreateUser (unique per request), GetUser, ListUsers
- [x] 3.5 STS (Query-XML): bench AssumeRole, GetCallerIdentity
- [x] 3.6 Kinesis (JSON): seed stream, bench PutRecord, DescribeStream, ListStreams
- [x] 3.7 Firehose (JSON): seed delivery stream, bench PutRecord, PutRecordBatch, ListDeliveryStreams
- [x] 3.8 SecretsManager (JSON): seed secret, bench CreateSecret, GetSecretValue, PutSecretValue, ListSecrets

## 4. Implement per-service benchmark sections (remaining 16)

- [x] 4.1 SQS (Query-XML/JSON): seed queue, bench SendMessage, ReceiveMessage, ListQueues
- [x] 4.2 KMS (JSON): seed key, bench Encrypt, Decrypt, ListKeys
- [x] 4.3 SSM (JSON): seed parameter, bench PutParameter, GetParameter, DescribeParameters
- [x] 4.4 ACM (JSON): bench RequestCertificate, ListCertificates
- [x] 4.5 CloudWatch (Query-XML): bench PutMetricData, GetMetricStatistics, ListMetrics
- [x] 4.6 EventBridge (JSON): seed rule, bench PutRule, ListRules, ListTargetsByRule
- [x] 4.7 Step Functions (JSON): seed state machine, bench StartExecution, ListStateMachines
- [x] 4.8 API Gateway (REST-JSON): seed REST API, bench CreateRestApi, GetRestApis
- [x] 4.9 EC2 (EC2-Query): bench DescribeInstances, DescribeVpcs
- [x] 4.10 Route53 (REST-XML): seed hosted zone, bench CreateHostedZone, ListHostedZones
- [x] 4.11 SES (Query-XML): bench VerifyEmailIdentity, ListIdentities
- [x] 4.12 ECR (JSON): seed repository, bench CreateRepository, DescribeRepositories
- [x] 4.13 OpenSearch (REST-JSON): bench CreateDomain, ListDomainNames
- [x] 4.14 Redshift (Query-XML): bench CreateCluster, DescribeClusters
- [x] 4.15 CloudFormation (Query-XML): seed stack, bench CreateStack, DescribeStacks
- [x] 4.16 Lambda (REST-JSON): seed function, bench Invoke, GetFunction, ListFunctions

## 5. Implement JSON report output

- [x] 5.1 Implement final JSON assembly: combine run metadata (profile, mode, timestamp, config), memory snapshots, and per-operation results into the output schema defined in design.md
- [x] 5.2 Handle skipped services: include skip entries with service name and failure reason in the results array
- [x] 5.3 Write JSON to `--output` path or stdout

## 6. Implement the benchmark gate script

- [x] 6.1 Create `tests/bench/bench_gate.sh` with argument parsing (`--report`, `--p95-threshold`, `--memory-budget`, `--output-markdown`)
- [x] 6.2 Implement per-operation p95 latency ratio evaluation: compare openstack p95 vs LocalStack p95, flag failures exceeding threshold
- [x] 6.3 Implement memory budget evaluation: compare openstack/LocalStack RSS ratio against budget
- [x] 6.4 Implement error rate check: fail if any openstack operation has non-zero errors
- [x] 6.5 Implement markdown summary generation: per-operation metrics table, memory comparison, overall PASS/FAIL verdict
- [x] 6.6 Implement exit codes: 0 for pass, 1 for failure, 2 for invalid input

## 7. Update CI workflows

- [x] 7.1 Update `.github/workflows/ci.yml`: replace `benchmark-smoke-fast` job with `bench_services.sh --profile smoke`, install oha, run gate, upload report artifact
- [x] 7.2 Update `.github/workflows/ci.yml`: replace `benchmark-smoke-full` job with `bench_services.sh --profile standard`, install oha, run gate, upload report artifact
- [x] 7.3 Update `.github/workflows/ci.yml`: update PR comment job to read new JSON report format and post gate markdown summary
- [x] 7.4 Update `.github/workflows/ci.yml`: remove `benchmark-gate-main`, `benchmark-gate-non-main`, and any baseline-fetching logic
- [x] 7.5 Rewrite `.github/workflows/benchmark-deep.yml` to use `bench_services.sh --profile deep`
- [x] 7.6 Remove `prepare-openstack-runtime-image` job if no longer needed by other workflow jobs (verify parity jobs don't depend on it)

## 8. Remove old benchmark system

- [x] 8.1 Delete `crates/tests/integration/src/benchmark.rs` and `crates/tests/integration/src/bin/benchmark_runner.rs`
- [x] 8.2 Delete `tests/benchmark/scenarios/*.json` (6 scenario files) and `tests/benchmark/` directory
- [x] 8.3 Delete `tests/bench/bench_startup.sh` and `tests/bench/bench_memory.sh`
- [x] 8.4 Delete `scripts/benchmark_report_tables.py`, `scripts/benchmark_regression_gate.py`, `scripts/benchmark_report_consolidated.py`, `scripts/benchmark_progress_dashboard.py`
- [x] 8.5 Remove benchmark-related module references from `crates/tests/integration/src/lib.rs` and any Cargo.toml binary entries for benchmark_runner
- [x] 8.6 Update `docs/act-benchmark-validation.md` and `docs/benchmark-optimization-backlog.md` to reference the new tooling

## 9. Verification (manual)

- [ ] 9.1 Run `bench_services.sh --profile smoke` locally against a running openstack Docker container and verify JSON output correctness
- [ ] 9.2 Run `bench_gate.sh` against the smoke report and verify markdown output and exit codes
- [ ] 9.3 Run `bench_services.sh --binary --profile smoke` locally and verify binary mode works with correct memory collection
- [ ] 9.4 Verify CI workflow runs benchmark jobs successfully on a test PR
- [ ] 9.5 Verify the project builds without errors after removing old benchmark code (`cargo build`, `cargo test`)
