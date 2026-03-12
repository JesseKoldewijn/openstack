# act Benchmark Workflow Validation

Use this playbook to validate benchmark and benchmark-gate behavior locally before pushing CI workflow changes.

## Prerequisites

- Docker running
- `act` installed (`act --version`)
- `GH_TOKEN` exported (required for baseline discovery through `gh`)

```bash
export GH_TOKEN=<github_token>
```

## Run non-main PR benchmark lane (fair-low-core)

```bash
act pull_request -W .github/workflows/ci.yml -j benchmark-smoke-fast --env GH_TOKEN="$GH_TOKEN"
```

Also run a parity lane in the same event context:

```bash
act pull_request -W .github/workflows/ci.yml -j parity-all-services-fast --env GH_TOKEN="$GH_TOKEN"
```

## Deterministic runtime-image checks

In each benchmark/parity job log, verify all of the following:

- `Prepare OpenStack runtime image` job ran and completed.
- `Using OpenStack runtime image: openstack-runtime-ci:<run-id>-<sha>` appears in consumer jobs.
- `Expected image id:` and `Actual image id:` are both present and equal.
- Runtime smoke check (`docker run --rm ... --version`) succeeds.

This confirms one run-scoped immutable runtime image reference is reused across lanes.

## Hosted CI validation (required before merge)

After pushing the workflow change branch, validate on GitHub Actions:

- Confirm `prepare-openstack-runtime-image` ran once in the workflow run.
- Confirm both a benchmark lane and a parity lane consumed the same runtime image id.
- Capture run URL and the matching image id lines as evidence in PR notes.

Expected outcomes:
- Benchmark job runs and emits report artifacts.
- Benchmark-gate step runs with diagnostics.
- If no baseline can be resolved in local simulation, gate fails with explicit diagnostic reason.

## Run main-target PR benchmark lane (fair-medium-core)

```bash
act pull_request -W .github/workflows/ci.yml -j benchmark-smoke-full --env GH_TOKEN="$GH_TOKEN"
```

## Gate pass/fail local script validation

Pass-path (same current and baseline):

```bash
python3 scripts/benchmark_regression_gate.py \
  --lane fair-low-core \
  --current target/benchmark-reports/fair-low-wave1-cross-target-validity.json \
  --previous target/benchmark-reports/fair-low-wave1-cross-target-validity.json \
  --strict-missing-baseline \
  --output-json target/benchmark-reports/benchmark-gate-fair-low-core-local-pass.json \
  --output-markdown target/benchmark-reports/benchmark-gate-fair-low-core-local-pass.md
```

Intentional fail-path (synthetic degraded baseline comparison):

```bash
python3 scripts/benchmark_regression_gate.py \
  --lane fair-low-core \
  --current target/benchmark-reports/gate-current-fail.json \
  --previous target/benchmark-reports/gate-baseline-fail.json \
  --strict-missing-baseline \
  --output-json target/benchmark-reports/benchmark-gate-fair-low-core-local-fail.json \
  --output-markdown target/benchmark-reports/benchmark-gate-fair-low-core-local-fail.md
```

## Troubleshooting

- `missing_gh_token`: export `GH_TOKEN` and rerun.
- `github_api_query_failed`: verify network access and GitHub API token scopes.
- `baseline_artifact_not_found`: verify workflow file and artifact name, or seed a new successful baseline run.
- `data_quality_no_valid_performance`: check benchmark report validity fields and invalid-reasons output.
- `Runtime image provenance mismatch`: verify producer artifact download/load steps and ensure no job retags/rebuilds the OpenStack runtime image.
