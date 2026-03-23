## Context

The `shell-benchmark-replacement` change replaced a ~7900-line Rust/Python benchmark system with two shell scripts: `tests/bench/bench_services.sh` (~1426 lines) and `tests/bench/bench_gate.sh` (~315 lines). The scripts use `oha` as the HTTP load generator. However, the oha integration is broken: the script passes `--json` (which doesn't exist in oha) instead of `--output-format json`, and the JSON extraction paths reference a non-existent schema. All metrics read as `0ms` / `0 RPS` in CI, making the gate pass trivially with no real data.

Additionally, the benchmark only compares openstack against LocalStack. Adding moto (a pure-Python AWS mock library with a standalone HTTP server) as a third target provides an independent comparison point. Moto is architecturally different from LocalStack — it's single-process Python without JVM components — making it useful for triangulating performance characteristics.

The Semgrep CI workflow is also redundant since CodeRabbit now provides the same analysis.

## Goals / Non-Goals

**Goals:**
- Fix oha invocation so benchmarks produce real metrics in CI
- Add moto (`motoserver/moto` Docker image) as a third benchmark target alongside openstack and LocalStack
- Make target selection configurable via `--targets os,ls,moto` flag (any subset allowed)
- Extend the gate to evaluate openstack performance against both LocalStack and moto
- Extend the markdown summary to show all three targets side-by-side
- Remove the Semgrep workflow
- Harden CI artifact download so missing benchmark artifacts don't block the PR comment job

**Non-Goals:**
- Changing the benchmark profile system (smoke/standard/deep remain as-is)
- Adding moto to the parity test system (parity is a separate concern)
- Historical trend tracking across benchmark runs
- Moto-specific service configuration or customization beyond default settings

## Decisions

### Decision 1: Fix oha flag and JSON paths

**Choice:** Replace `--json` with `--output-format json` and update all JSON extraction paths.

**Current (broken):**
```bash
oha -n "$REQ_COUNT" -c "$CONC" -m "$method" --json --no-tui ...
p50=$(jq -r '.responseTimeHistogram.percentiles."50" // 0' ...)
p95=$(jq -r '.responseTimeHistogram.percentiles."95" // 0' ...)
p99=$(jq -r '.responseTimeHistogram.percentiles."99" // 0' ...)
throughput=$(jq -r '.summary.requestsPerSec // 0' ...)
```

**Fixed:**
```bash
oha -n "$REQ_COUNT" -c "$CONC" -m "$method" --output-format json --no-tui ...
p50=$(jq -r '.latencyPercentiles.p50 // 0' ...)
p95=$(jq -r '.latencyPercentiles.p95 // 0' ...)
p99=$(jq -r '.latencyPercentiles.p99 // 0' ...)
throughput=$(jq -r '.summary.requestsPerSec // 0' ...)
```

**Rationale:** Verified against oha 1.14.0's actual JSON output. The `.summary.requestsPerSec` path is already correct. The percentile paths move from the non-existent `responseTimeHistogram.percentiles` to `latencyPercentiles`. Values are in seconds and need `*1000` conversion to milliseconds (already handled by the existing awk pipe).

**Alternative considered:** None — this is a pure bug fix.

### Decision 2: Moto container lifecycle

**Choice:** Run moto as a Docker container with the same CPU/memory constraints as openstack and LocalStack, using the `motoserver/moto` official image.

**Container configuration:**
- Image: `motoserver/moto:latest` (configurable via `--moto-image`)
- Port: `5555` (moto's default) mapped to host
- CPU/memory limits: Same as other containers (`PARITY_DOCKER_CPU_LIMIT` / `PARITY_DOCKER_MEMORY_LIMIT`)
- Health check: `GET http://localhost:5555/moto-api/` returns 200
- Startup timeout: 30 seconds (moto starts quickly, it's pure Python)

**Rationale:** Moto's standalone server mode (`moto_server`) serves all AWS services on a single port, auto-routing based on request headers — exactly like LocalStack. The `motoserver/moto` Docker image is the official distribution. Using the same resource constraints as the other containers keeps the comparison fair.

**Alternative considered:** Running moto via `pip install moto[server] && moto_server`. Rejected because Docker provides consistent isolation and resource constraints matching the other targets.

### Decision 3: Target selection via `--targets` flag

**Choice:** Add `--targets <comma-separated>` flag defaulting to `os,ls,moto`. The flag controls which targets are started and benchmarked.

**Behavior:**
- `--targets os,ls` — only start openstack + LocalStack (current behavior, no moto)
- `--targets os,moto` — only start openstack + moto
- `--targets os,ls,moto` — start all three (default)
- At least `os` must always be present (error if omitted)
- Container startup, health checks, benchmark calls, memory collection, and cleanup are all conditional on the target being in the list
- JSON report includes only the targets that were actually benchmarked

**Rationale:** Allows flexible benchmarking — users can run quick two-target comparisons locally, while CI runs all three. Also allows graceful degradation if moto image is unavailable.

### Decision 4: Extend `bench_both()` to `bench_targets()`

**Choice:** Rename `bench_both()` to `bench_targets()` and have it iterate over all active targets.

**Current:**
```bash
bench_both() {
  local service="$1" operation="$2" method="$3" os_url="$4" ls_url="$5"
  shift 5; local extra_args=("$@")
  bench "$service" "$operation" "os" "$method" "$os_url" "${extra_args[@]}"
  bench "$service" "$operation" "ls" "$method" "$ls_url" "${extra_args[@]}"
}
```

**New:**
```bash
bench_targets() {
  local service="$1" operation="$2" method="$3" os_url="$4" ls_url="$5" moto_url="$6"
  shift 6; local extra_args=("$@")
  [[ "$TARGETS" == *os* ]]   && bench "$service" "$operation" "os" "$method" "$os_url" "${extra_args[@]}"
  [[ "$TARGETS" == *ls* ]]   && bench "$service" "$operation" "ls" "$method" "$ls_url" "${extra_args[@]}"
  [[ "$TARGETS" == *moto* ]] && bench "$service" "$operation" "moto" "$method" "$moto_url" "${extra_args[@]}"
}
```

Each per-service section's call changes from:
```bash
bench_both "s3" "put_object" "PUT" "$OS_BASE/..." "$LS_BASE/..." -H "..." -d '...'
```
to:
```bash
bench_targets "s3" "put_object" "PUT" "$OS_BASE/..." "$LS_BASE/..." "$MOTO_BASE/..." -H "..." -d '...'
```

**Rationale:** Minimal change to existing call sites — just add the moto URL parameter. The conditional checks on `TARGETS` skip inactive targets without needing separate control flow.

**For `bench()` itself:** Add `"moto"` as a valid target mapping: `[[ "$target" == "moto" ]] && target_field="moto"`.

### Decision 5: JSON report schema extension

**Choice:** Add `moto` fields to both the memory section and per-operation results.

```json
{
  "memory": {
    "openstack": { "idle_mb": 1.9, "loaded_mb": 3.2 },
    "localstack": { "idle_mb": 280, "loaded_mb": 710 },
    "moto": { "idle_mb": 45, "loaded_mb": 120 }
  },
  "results": [
    {
      "service": "s3",
      "operation": "put_object",
      "openstack": { "p50_ms": 2.1, "p95_ms": 4.3, ... },
      "localstack": { "p50_ms": 12.3, "p95_ms": 18.7, ... },
      "moto": { "p50_ms": 8.5, "p95_ms": 14.2, ... }
    }
  ]
}
```

Fields for targets not included in `--targets` are simply absent from the JSON.

### Decision 6: Gate evaluation with moto

**Choice:** The gate evaluates openstack against each active comparison target independently. If either ratio exceeds the threshold, the gate fails.

**Evaluation logic:**
- For each operation, compute `os_p95 / ls_p95` AND `os_p95 / moto_p95` (when both targets are present)
- Either ratio exceeding `--p95-threshold` triggers a failure
- Memory budget check: compare openstack loaded RSS against each comparison target's loaded RSS
- Error check: unchanged (zero tolerance for openstack errors)

**Markdown table extension:**
```
| Service | Op | OS p50 | OS p95 | OS p99 | LS p95 | Moto p95 | OS/LS | OS/Moto | OS RPS | LS RPS | Moto RPS | Status |
```

This is wider than before but keeps all data visible in one row per operation.

### Decision 7: Moto URL patterns

**Choice:** Moto uses the same URL patterns and headers as LocalStack for most services.

Moto's standalone server routes based on:
- `X-Amz-Target` header for JSON services (DynamoDB, Kinesis, etc.)
- URL path + `Action` query parameter for Query services (IAM, STS, etc.)
- URL path for REST services (S3, Route53, etc.)

This means the moto URL for each operation is typically identical to the LocalStack URL but with a different base (e.g., `http://localhost:5555` instead of `http://localhost:4566`). The per-service sections just need to substitute `MOTO_BASE` for `LS_BASE`.

**Exception:** S3 path-style URLs work on moto. Virtual-hosted-style may not without additional configuration. We'll use path-style for all S3 operations (consistent with how we already call LocalStack).

### Decision 8: Semgrep removal and CI hardening

**Choice:** Delete `.github/workflows/semgrep.yml` and add `continue-on-error: true` to the benchmark artifact download step in `ci.yml`.

**Rationale:** CodeRabbit handles Semgrep analysis. The `continue-on-error` prevents the PR comment job from failing when a benchmark job was skipped or failed before uploading artifacts — the Python script already handles missing files gracefully with `if gate_md.exists()`.

## Risks / Trade-offs

**[Moto service coverage gaps]** → Some services supported by LocalStack may not be fully implemented in moto. Mitigation: The `bench()` function already handles errors gracefully (non-2xx counted as errors). Services where moto returns errors will show in the report. The `skip_service()` mechanism can handle seed failures.

**[Moto performance characteristics differ from LocalStack]** → Moto is pure Python with no JVM/Go components. It may be significantly faster or slower than LocalStack for certain operations. Mitigation: This is actually the point — having two independent comparison targets gives a more complete picture.

**[Wider markdown tables]** → Adding 4 more columns (Moto p95, Moto RPS, OS/Moto ratio, Moto p50) makes the table harder to read on narrow screens. Mitigation: The table is already 12 columns; going to ~15 is manageable in GitHub PR comments which have horizontal scroll. We can consider a condensed format later if needed.

**[CI time increase]** → Running benchmarks against a third target adds ~30-50% more benchmark execution time. Mitigation: The moto container starts fast (~5s vs LocalStack's ~30s). The actual HTTP benchmarks are the main time cost, and with smoke profile's 50 requests per operation, the added moto calls are minimal (~1-2 minutes total).

**[Moto Docker image size]** → `motoserver/moto` is ~500MB. Mitigation: Docker layer caching in CI handles this. The image pull only needs to happen once per workflow run.

## Risks / Trade-offs

**[oha version compatibility]** → The JSON output schema was verified against oha 1.14.0. Future versions may change field names. Mitigation: Pin a specific oha version in CI download URL instead of using `latest`. Add a version check in the script.

## Migration Plan

1. Fix oha invocation (no migration needed — pure bug fix)
2. Add moto support with `--targets` flag defaulting to `os,ls,moto`
3. Update CI workflows to pull moto image and pass `--targets os,ls,moto`
4. Update gate script for three-target evaluation
5. Delete Semgrep workflow
6. Push and verify CI produces real metrics

Rollback: Revert the commits. The `--targets os,ls` flag ensures moto can be disabled without code changes if issues arise.

## Open Questions

- Should we pin a specific moto Docker image version (e.g., `motoserver/moto:5.x.x`) like we do for LocalStack (`localstack/localstack:3.7.2`), or use `latest`?
- For services where moto returns 5xx or is not implemented, should the gate treat moto failures differently from LocalStack failures (e.g., skip the moto ratio check for that operation)?
