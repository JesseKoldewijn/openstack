# Native HTTP validation notes

## Baseline inventory

- `README.md` lists 24 supported services.
- `tests/harness/service-matrix.json` includes the same 24 services.
- `tests/parity/scenarios/all-services-smoke.json` includes one baseline parity probe for each of the 24 services.
- `tests/benchmark/scenarios/all-services-smoke.json` includes one baseline benchmark probe for each of the 24 services.
- The fast smoke profiles remain intentionally narrower and are not the authoritative README baseline.

## Probe-contract decision

- The all-services smoke scenarios remain the baseline migration contract for this change.
- These scenarios are intentionally probe-style and primarily verify native transport reachability plus LocalStack-referenced equivalence for representative error or lookup paths.
- Richer lifecycle coverage remains concentrated in core/deep scenarios and follow-up work should expand service-specific lifecycle semantics without replacing the 24-service smoke inventory.

## Runtime validation completed

- Core parity profile executed successfully under native HTTP.
- Latest core report: `target/parity-reports/core-latest.json`
- Core parity result after normalization fixes: 3/5 passed, 2/5 failed.
- Core parity passes: `s3`, `sqs`, `sts`.
- Core parity remaining mismatches:
  - `dynamodb`: missing-table error `content-type` differs from LocalStack.
  - `compatibility`: disabled-service SNS check returns `500` in openstack vs `501` in LocalStack.

- All-services parity profile executed successfully under native HTTP.
- Latest all-services report: `target/parity-reports/all-services-smoke-latest.json`
- All-services parity result: 2/24 passed, 22/24 failed.
- README baseline services currently equivalent under native HTTP smoke parity:
  - `sqs`
  - `sts`
- README baseline services currently reported as machine-readable follow-up-required:
  - `acm`
  - `apigateway`
  - `cloudformation`
  - `cloudwatch`
  - `dynamodb`
  - `ec2`
  - `ecr`
  - `events`
  - `firehose`
  - `iam`
  - `kinesis`
  - `kms`
  - `lambda`
  - `opensearch`
  - `redshift`
  - `route53`
  - `s3`
  - `secretsmanager`
  - `ses`
  - `sns`
  - `ssm`
  - `states`

- Representative benchmark lane executed successfully under native HTTP.
- Latest benchmark report: `target/benchmark-reports/fair-low-core-latest.json`
- Benchmark runtime confirms `execution_driver: native-http` with no hidden CLI fallback.
- Benchmark lane result highlights:
  - 8 scenarios total
  - 7 valid performance scenarios
  - 1 invalid scenario (`sts-core-call: unknown scenario role`)
  - 0 openstack errors
  - 0 localstack errors
  - 9 missing required benchmark roles

## Follow-up inventory

- Concrete follow-up work is recorded in `openspec/changes/native-http-parity-benchmarks/follow-up-items.md`.
- No accepted differences are currently recorded in `tests/parity/known_differences.json`.

## CI and harness follow-up completed here

- Removed AWS CLI installation and AWS CLI preflight checks from parity and benchmark jobs in `.github/workflows/ci.yml`.
- Removed AWS CLI installation and AWS CLI preflight checks from `.github/workflows/benchmark-deep.yml`.
- Updated `scripts/benchmark_regression_gate.py` to require `execution_driver=native-http` instead of `direct-http`.
