## Context

The current benchmark system has three layers: a ~3000-line Rust integration test module that runs benchmark scenarios via native HTTP, a Python reporting/gating pipeline (~2000 lines across 4 scripts), and two lightweight shell scripts for startup/memory measurement. This system was built iteratively — each layer added sophistication (classification systems, role coverage, lane interpretability, weighted aggregates, baseline regression tracking) that made the system comprehensive but opaque and brittle.

The CI regression gate currently requires a prior successful baseline run to exist as a GitHub Actions artifact. This blocks new branches and fresh repos from passing CI. The weighted-average reporting obscures individual service performance behind aggregate numbers.

We are replacing this entire stack with a single shell-based benchmark script and a simplified gate script.

## Goals / Non-Goals

**Goals:**
- Single shell script (`tests/bench/bench_services.sh`) that benchmarks all 24 services with ~3-4 operations each
- Profile system: `smoke` (8 core, light), `standard` (all 24, medium), `deep` (all 24, heavy)
- Dual runtime mode: Docker containers (fair comparison, default) and bare binary (`--binary`, openstack-only showcase)
- Raw per-operation metrics output (p50, p95, p99, throughput, errors) — no weighted averages
- Standalone gate that compares openstack vs LocalStack within the same run — no prior baseline dependency
- Works identically locally and in CI
- Complete removal of Rust benchmark engine, Python pipeline, and old shell scripts

**Non-Goals:**
- Historical trend tracking across runs (can be added later as a separate concern)
- Weighted or aggregated cross-service metrics
- Service classification systems (execution class, durability class, envelope validation)
- S3 heavy-object benchmarks (1GB/5GB/10GB) — defer to future work
- Lambda invoke benchmarks requiring Docker-in-Docker runtime — defer if too complex

## Decisions

### Decision 1: oha as primary HTTP bench tool, hey as fallback

**Choice:** Use `oha` for benchmarking, fall back to `hey` if `oha` is not available.

**Rationale:** `oha` provides native `--json` output with structured percentile data (p50, p95, p99, min, max, mean, stddev) and throughput. This eliminates fragile text parsing. `hey` requires parsing human-readable text output with awk/sed to extract the same metrics, making the bench() function more complex and brittle.

**Fallback strategy:** The script auto-detects which tool is available at startup. The `bench()` helper function abstracts the tool-specific invocation and output parsing, so the per-service sections are tool-agnostic.

**Alternative considered:** `ab` (Apache Bench) — universally available but lacks JSON output and modern HTTP features. `wrk` — requires Lua scripting for POST requests with custom headers. Neither fits as well as oha.

### Decision 2: Explicit per-service HTTP calls, no protocol abstraction layer

**Choice:** Each service section in the script makes explicit HTTP calls with the correct protocol headers (Content-Type, X-Amz-Target, URL paths). No abstraction layer maps "service + operation" to protocol details.

**Rationale:** Transparency is a primary goal. Anyone reading the script should immediately see what HTTP request each benchmark sends. Protocol abstraction would recreate the opacity problem we're escaping from. The script is long (~800-1000 lines) but every line is obvious.

**Trade-off:** Adding a new service or operation requires knowing its AWS protocol details and writing the curl-like invocation by hand. This is acceptable because services change rarely and the explicit approach prevents silent protocol mismatches.

### Decision 3: Docker-first with optional bare binary mode

**Choice:** Default mode starts both openstack and LocalStack in Docker containers with identical resource constraints (CPU, memory). The `--binary` flag skips Docker for openstack and runs the bare binary directly, while still running LocalStack in Docker.

**Docker mode (default):**
- Both containers get identical CPU/memory limits (configurable, default 2 CPU / 4GB)
- Fair comparison — same overhead for both targets
- Memory comparison via `docker stats` RSS

**Binary mode (`--binary`):**
- OpenStack runs as a bare process — no Docker overhead
- LocalStack still runs in Docker (it requires Docker)
- Memory comparison uses `/proc/<pid>/status` VmRSS for openstack, `docker stats` for LocalStack
- Results are intentionally asymmetric — this mode showcases native Rust binary performance

**Alternative considered:** Binary-only mode (no LocalStack comparison). Rejected because even in showcase mode, having the LocalStack baseline tells the story more compellingly.

### Decision 4: Profile system with service and load filtering

**Choice:** Three built-in profiles plus ad-hoc filtering:

| Profile    | Services               | Requests | Concurrency | Use case            |
|------------|------------------------|----------|-------------|---------------------|
| `smoke`    | 8 core parity services | 50       | 1           | PRs to non-main     |
| `standard` | All 24 services        | 200      | 2           | PRs to main         |
| `deep`     | All 24 services        | 1000     | 4           | Nightly / scheduled  |

Additional filtering via `--services s3,dynamodb,iam` for ad-hoc runs. `--requests` and `--concurrency` flags override profile defaults.

### Decision 5: JSON output schema — flat, raw, per-operation

**Choice:** Output a JSON report with raw per-operation metrics. No weighted averages, no aggregation.

```json
{
  "profile": "standard",
  "mode": "docker",
  "timestamp": "2026-03-19T12:00:00Z",
  "config": {
    "requests": 200,
    "concurrency": 2,
    "openstack_image": "...",
    "localstack_image": "...",
    "cpu_limit": "2",
    "memory_limit": "4g"
  },
  "memory": {
    "openstack": { "idle_mb": 1.9, "loaded_mb": 3.2 },
    "localstack": { "idle_mb": 280, "loaded_mb": 710 }
  },
  "results": [
    {
      "service": "s3",
      "operation": "put_object",
      "openstack": {
        "p50_ms": 2.1, "p95_ms": 4.3, "p99_ms": 6.1,
        "throughput_rps": 450.2, "errors": 0, "total": 200
      },
      "localstack": {
        "p50_ms": 12.3, "p95_ms": 18.7, "p99_ms": 24.1,
        "throughput_rps": 78.5, "errors": 0, "total": 200
      }
    }
  ]
}
```

In binary mode, the `localstack` field is present when LocalStack is running, and `config.mode` is `"binary"`.

### Decision 6: Gate evaluates within-run ratios only

**Choice:** The gate script reads the JSON report and checks per-operation openstack-vs-LocalStack ratios within the same run. No prior baseline lookup.

**Gate criteria:**
- Per-operation p95 latency ratio: openstack must not exceed LocalStack by more than a configurable threshold (default: openstack p95 <= 1.5x LocalStack p95)
- Memory budget: openstack RSS must remain below a configurable ratio of LocalStack RSS (default: 0.20)
- Error rate: openstack error rate must be 0% for all operations

**Rationale:** The prior-baseline approach is fundamentally broken for new branches, forks, and fresh repos. Within-run comparison is self-contained and always works. If openstack is faster than LocalStack, it passes. Simple.

### Decision 7: Replace CI workflow jobs entirely

**Choice:** Rewrite the benchmark-related jobs in `ci.yml` and `benchmark-deep.yml`:

- `benchmark-smoke-fast` (PRs to non-main) → `./tests/bench/bench_services.sh --profile smoke --output report.json`
- `benchmark-smoke-full` (PRs to main) → `./tests/bench/bench_services.sh --profile standard --output report.json`
- `benchmark-smoke-push` (push to main) → removed or made informational
- `benchmark-deep.yml` (nightly) → `./tests/bench/bench_services.sh --profile deep --output report.json`
- Gate jobs invoke the new gate script on the report JSON
- PR comment job reads the report JSON and formats a markdown summary

## Risks / Trade-offs

**[Less statistical rigor]** → The Rust engine had warmup iterations, controlled measurement phases, and stddev tracking. The shell script delegates statistical collection to oha/hey. Mitigation: oha's built-in statistics (percentiles, stddev) are sufficient for the comparison use case.

**[Long shell script]** → ~800-1000 lines of bash with 24 service sections is substantial. Mitigation: Each section is self-contained and follows an identical pattern. No complex control flow or abstraction. Easy to grep for a specific service.

**[oha/hey availability]** → These tools are not pre-installed on GitHub Actions runners. Mitigation: CI installs oha via cargo-binstall or binary download in the workflow. Document local installation in README.

**[Binary mode comparison is asymmetric]** → Comparing a bare binary to a Docker container is not a fair benchmark. Mitigation: This mode is clearly labeled and not used in CI gates. It's a separate showcase mode with its own output section.

**[Loss of historical trend data]** → Removing baseline comparison means we can't detect gradual regressions across PRs. Mitigation: Within-run comparison against LocalStack is a stable external baseline. Trend tracking can be added later by storing report JSONs and comparing across runs if needed.

**[Service seed failures]** → Some services need setup (create table, create bucket, create stream) before benchmarking. If seed fails, the entire service section fails. Mitigation: Each service section handles its own seed with error checking and skips gracefully if seed fails, recording the skip in the JSON output.

## Migration Plan

1. Add new `tests/bench/bench_services.sh` and gate script alongside existing system
2. Update CI workflows to use new scripts
3. Verify CI passes with new benchmark system
4. Remove old Rust benchmark engine, Python scripts, and old shell scripts
5. Remove old benchmark scenario JSON files
6. Update documentation

Rollback: revert the CI workflow changes and restore the old benchmark jobs. The old Rust code can be recovered from git history.

## Open Questions

- Should the PR comment format be a simple markdown table of raw metrics, or include a condensed pass/fail summary with expandable details?
- For services where seed operations are non-trivial (Lambda function creation, Step Functions state machine), should we skip those services in the smoke profile entirely or attempt a fast seed?
- Should the `--binary` mode output be a separate report file or included in the same JSON with a different mode field?
