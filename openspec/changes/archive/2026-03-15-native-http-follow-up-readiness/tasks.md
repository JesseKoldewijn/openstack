## 1. Parity Baseline Governance

- [x] 1.1 Remove stale runtime-blocker text from `openspec/changes/native-http-parity-benchmarks/validation-notes.md` so readiness evidence reflects the validated native HTTP state.
- [x] 1.2 Audit the 22 README baseline follow-up-required parity services and group each mismatch into fix, accepted-difference candidate, or still-unresolved follow-up buckets.
- [x] 1.3 Decide and record governance for cross-cutting parity policy questions, including DynamoDB error `content-type` handling and the disabled-service compatibility mismatch.

## 2. High-Signal Parity Corrections

- [x] 2.1 Fix or govern the remaining core parity mismatches so core results reflect only intentional accepted differences.
- [x] 2.2 Correct high-signal all-services smoke probe behaviors where openstack currently returns success but LocalStack returns an error (for example CloudFormation, CloudWatch, and EC2 probe paths).
- [x] 2.3 Correct service-specific missing-resource and failure semantics for the validated README baseline follow-up services or record accepted differences where intentional equivalence is not yet practical.
- [x] 2.4 Re-run core and all-services smoke parity profiles and refresh the per-service readiness evidence after parity corrections.

## 3. Benchmark Signal-Quality Completion

- [x] 3.1 Add missing write-role fair-core benchmark workloads for services currently covered only by read scenarios, or record explicit auditable exclusions where workloads are not yet ready.
- [x] 3.2 Resolve the `sts` role-classification gap by introducing explicit read/write benchmark roles or a documented exclusion instead of relying on `aux` coverage.
- [x] 3.3 Improve benchmark diagnostics handling for missing runtime evidence, including the current missing OpenStack RSS measurement path.
- [x] 3.4 Re-run representative native benchmark lanes and confirm role coverage, invalid reasons, and interpretability summaries match the intended contract.

## 4. Readiness Evidence and Closure

- [x] 4.1 Update `openspec/changes/native-http-parity-benchmarks/follow-up-items.md` and related notes to reflect which gaps were fixed, which became accepted differences, and which remain explicit follow-up-required outcomes.
- [x] 4.2 Ensure parity and benchmark reports, documentation, and governance artifacts describe the all-services smoke profile as the authoritative README baseline contract.
- [x] 4.3 Capture final readiness evidence showing the current equivalent services, accepted differences, remaining follow-up-required services, and benchmark role/exclusion state.
