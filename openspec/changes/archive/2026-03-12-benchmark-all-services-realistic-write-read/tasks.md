## 1. Service Workload Matrix Foundation

- [x] 1.1 Add benchmark scenario role metadata (write/read/admin/aux) to the benchmark scenario model and serialization
- [x] 1.2 Define machine-readable service workload matrix structure mapping each supported service to required write and read roles
- [x] 1.3 Populate matrix entries for all services currently returned by `all_service_names()`
- [x] 1.4 Add validation that every supported service has a matrix entry at benchmark startup
- [x] 1.5 Add validation that matrix entries include at least one required write role and one required read role

## 2. Realistic Scenario Packs for All Services

- [x] 2.1 Replace/augment default all-services benchmark scenario generation to produce realistic write and read scenarios per service
- [x] 2.2 Create deterministic setup/cleanup lifecycle steps for services that require resource provisioning before measured operations
- [x] 2.3 Add explicit scenario role/category metadata to every all-services realistic scenario
- [x] 2.4 Update external scenario fixtures in `tests/benchmark/scenarios/` to reflect realistic write/read contract where applicable
- [x] 2.5 Implement service-specific wait/readiness handling for eventually consistent APIs to reduce benchmark flakiness
- [x] 2.6 Ensure resource names remain deterministic and run-scoped to avoid cross-scenario collisions

## 3. Coverage Completeness and Signal-Quality Enforcement

- [x] 3.1 Implement per-service write/read completeness evaluation during benchmark result summarization
- [x] 3.2 Mark lanes non-interpretable when any required service lacks valid write-role scenario results
- [x] 3.3 Mark lanes non-interpretable when any required service lacks valid read-role scenario results
- [x] 3.4 Add machine-readable invalid reasons for missing-role coverage and unknown scenario roles
- [x] 3.5 Add exclusion reason model keyed by service and role with deterministic reason codes
- [x] 3.6 Surface per-service role completeness, exclusions, and invalid reasons in benchmark report output

## 4. Runtime Envelope Metrics

- [x] 4.1 Add startup timing measurement support with repeated sampling per benchmark target
- [x] 4.2 Add idle memory snapshot collection for openstack and LocalStack targets
- [x] 4.3 Add post-load memory snapshot collection for openstack and LocalStack targets
- [x] 4.4 Add runtime envelope comparison fields (startup and memory) to benchmark report schema/output
- [x] 4.5 Add explicit missing-envelope diagnostics when collection is unavailable for a target

## 5. Profile and Lane Strategy Updates

- [x] 5.1 Define/adjust required all-services realistic profiles to preserve CI runtime budgets while enforcing full write/read coverage
- [x] 5.2 Keep deeper workload profiles for high-impact services and ensure they remain non-blocking where intended
- [x] 5.3 Ensure profile matching logic maps realistic scenarios into the correct required and deep lanes
- [x] 5.4 Add benchmark execution documentation describing realistic lane intent, runtime cost, and troubleshooting

## 6. Regression Gate Integration

- [x] 6.1 Extend benchmark gate validation to fail required lanes when any required service is missing valid write coverage
- [x] 6.2 Extend benchmark gate validation to fail required lanes when any required service is missing valid read coverage
- [x] 6.3 Emit machine-readable gate diagnostics listing affected services and missing roles
- [x] 6.4 Keep threshold checks backward-compatible while incorporating new completeness preconditions

## 7. Test Coverage and Validation

- [x] 7.1 Add unit tests for service workload matrix validation and role completeness logic
- [x] 7.2 Add unit tests for unknown-role and exclusion reason handling
- [x] 7.3 Add unit tests for runtime envelope metric parsing and report emission
- [x] 7.4 Update benchmark summary/report tests to assert per-service realistic coverage diagnostics
- [x] 7.5 Run benchmark profiles locally to verify all services have measured write+read scenarios or explicit exclusions
- [x] 7.6 Run regression gate script against representative reports to verify strict completeness failure behavior

## 8. Rollout and Safety Controls

- [x] 8.1 Implement diagnostics-first mode (visibility phase) for realistic coverage checks
- [x] 8.2 Add configuration switch or rollout guard for moving from diagnostics-only to strict gate enforcement
- [x] 8.3 Document rollback path for reverting strict completeness enforcement if lane stability regresses
