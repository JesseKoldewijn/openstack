# Native HTTP follow-up items

## Baseline parity equivalence status

### Fixed in this follow-up

- Core parity is now fully equivalent: latest `target/parity-reports/core-latest.json` reports `5/5 passed`, `0 failed`, `0 accepted differences`.
- README-baseline all-services smoke parity is now fully equivalent: latest `target/parity-reports/all-services-smoke-latest.json` reports `24/24 passed`, `0 failed`, `0 accepted differences`.
- The former high-signal and service-specific parity gaps called out in this follow-up were resolved directly in provider logic, translator routing, or probe selection rather than governed away.
- OpenSearch specifically was resolved by restoring real `ListDomainNames` success semantics, aligning OpenSearch signing/service detection with `es` credential scope, and keeping the smoke baseline authoritative without adding accepted differences.

### Accepted differences

- None. `tests/parity/known_differences.json` remains empty.

### Remaining parity follow-up-required outcomes

- None in the core profile.
- None in the 24-service all-services smoke README baseline profile.

## Cross-cutting parity follow-up

- DynamoDB policy was applied as intended: the missing-table error `content-type` mismatch was fixed in openstack rather than governed or normalized away.
- Disabled-service compatibility policy was applied as intended: the SNS disabled-service `500` vs LocalStack `501` mismatch was fixed in the compatibility layer rather than treated as an accepted difference.
- Normalization policy: keep protocol-aware normalization limited to non-semantic wire noise only; do not normalize status codes, error families, or materially different error bodies.
- README-baseline policy: keep the 24-service all-services smoke profile as the authoritative readiness contract even when richer lifecycle scenarios pass elsewhere.

## Probe-contract follow-up

- Keep the 24-service all-services smoke scenarios as the authoritative migration baseline.
- The latest all-services smoke baseline now passes completely without accepted differences.
- Add richer lifecycle parity scenarios later for services currently represented only by probe-style failure/look-up checks.

## Benchmark signal-quality follow-up

- Add missing write-role workloads for `dynamodb`, `firehose`, `iam`, `kinesis`, `s3`, `secretsmanager`, and `sns` in the fair core benchmark lanes.
- Decide whether `sts` should gain explicit read/write benchmark roles or be excluded with documented role exclusions instead of remaining `aux` only.
- Investigate missing openstack memory RSS collection in the benchmark report so memory ratio reporting becomes complete.
