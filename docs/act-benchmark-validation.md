# act Benchmark Workflow Validation

Use this playbook to validate benchmark and benchmark-gate behavior locally before pushing CI workflow changes.

## Prerequisites

- Docker running
- `act` installed (`act --version`)
- `oha` installed (`oha --version`) — install from <https://github.com/hatoo/oha>
- `jq` installed (`jq --version`)

## Run non-main PR benchmark lane (smoke profile)

```bash
act pull_request -W .github/workflows/ci.yml -j benchmark-smoke-fast
```

Also run a parity lane in the same event context:

```bash
act pull_request -W .github/workflows/ci.yml -j parity-all-services-fast
```

## Run main-target PR benchmark lane (standard profile)

```bash
act pull_request -W .github/workflows/ci.yml -j benchmark-smoke-full
```

## Run deep benchmark locally

```bash
./tests/bench/bench_services.sh --profile deep --output benchmark-report.json
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
- Benchmark job runs `bench_services.sh` and emits `benchmark-report.json`.
- Gate step runs `bench_gate.sh` and emits `benchmark_gate_summary.md`.
- Gate summary is appended to the workflow step summary.

## Gate pass/fail local validation

Pass-path (run gate on a report):

```bash
./tests/bench/bench_gate.sh \
  --report benchmark-report.json \
  --output-markdown benchmark_gate_summary.md
echo "Exit code: $?"
```

Adjust thresholds to test failure paths:

```bash
./tests/bench/bench_gate.sh \
  --report benchmark-report.json \
  --p95-threshold 0.5 \
  --output-markdown benchmark_gate_summary.md
echo "Exit code: $?"
```

## Troubleshooting

- `oha: command not found`: install oha — `curl -sSfL https://github.com/hatoo/oha/releases/latest/download/oha-linux-amd64 -o /usr/local/bin/oha && chmod +x /usr/local/bin/oha`
- `jq: command not found`: install jq via package manager (`apt install jq`, `brew install jq`).
- `Runtime image provenance mismatch`: verify producer artifact download/load steps and ensure no job retags/rebuilds the OpenStack runtime image.
