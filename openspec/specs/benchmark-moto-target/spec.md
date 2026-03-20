## Purpose
Moto as a third benchmark comparison target alongside openstack and LocalStack.

## Requirements

### Requirement: Moto container lifecycle management
The benchmark system SHALL manage a moto Docker container (`motoserver/moto`) as a third benchmark target alongside openstack and LocalStack, with equivalent resource constraints and health verification.

#### Scenario: Moto container starts with equivalent resource constraints
- **WHEN** moto is included in the active target set
- **THEN** the harness SHALL start a `motoserver/moto` Docker container with the same CPU and memory limits applied to openstack and LocalStack, mapping port 5555 to the host

#### Scenario: Moto container health is verified before benchmarking
- **WHEN** the moto container is started
- **THEN** the harness SHALL poll `GET http://localhost:5555/moto-api/` until a 200 response is received or a 30-second timeout expires, and SHALL abort with a diagnostic message on timeout

#### Scenario: Moto container is cleaned up after benchmarking
- **WHEN** benchmark execution completes or is interrupted
- **THEN** the harness SHALL stop and remove the moto container as part of cleanup

#### Scenario: Moto container is not started when excluded from targets
- **WHEN** moto is not included in the `--targets` flag
- **THEN** the harness SHALL NOT start a moto container, and no moto health checks, benchmark calls, or memory collection SHALL occur

### Requirement: Moto benchmark execution for all services
The benchmark system SHALL execute benchmark operations against moto using the same HTTP request patterns used for LocalStack, substituting only the base URL.

#### Scenario: Each service operation is benchmarked against moto
- **WHEN** moto is an active target and a service benchmark section executes
- **THEN** the harness SHALL call `bench_targets()` with a moto URL derived from `MOTO_BASE` (http://localhost:5555) using the same path, headers, and body as the LocalStack request

#### Scenario: Moto uses path-style S3 URLs
- **WHEN** S3 operations are benchmarked against moto
- **THEN** the harness SHALL use path-style URLs (not virtual-hosted-style) for all S3 operations against moto, and SHALL set a `Host: s3.amazonaws.com` header via `MOTO_EXTRA` for correct moto S3 routing

#### Scenario: Moto S3 operations include required auth header via MOTO_EXTRA
- **WHEN** S3 operations are benchmarked against moto
- **THEN** the harness SHALL append a static dummy `Authorization: AWS4-HMAC-SHA256 ...` header via the `MOTO_EXTRA` array, required by moto 5.x for GET/HEAD object operations (without it moto returns 403); the signature value is not validated so a static dummy is sufficient. `MOTO_EXTRA` SHALL be set at the start of the S3 service block and cleared after it.

#### Scenario: Moto errors are captured in benchmark results
- **WHEN** moto returns non-2xx responses for a benchmark operation
- **THEN** the harness SHALL record the errors in the moto results for that operation without aborting the benchmark run

### Requirement: Moto metrics in JSON report
The benchmark JSON report SHALL include moto metrics alongside openstack and LocalStack metrics when moto is an active target.

#### Scenario: JSON report includes moto memory metrics
- **WHEN** moto is an active target and the benchmark run completes
- **THEN** the JSON report `memory` section SHALL include a `moto` object with `idle_mb` and `loaded_mb` fields

#### Scenario: JSON report includes per-operation moto results
- **WHEN** moto is an active target and an operation benchmark completes
- **THEN** the JSON report results array SHALL include a `moto` object for that operation with `p50_ms`, `p95_ms`, `p99_ms`, `rps`, and `errors` fields

#### Scenario: JSON report omits moto fields when moto is not active
- **WHEN** moto is not included in the active target set
- **THEN** the JSON report SHALL NOT contain `moto` keys in the memory section or per-operation results

### Requirement: Configurable target selection
The benchmark system SHALL support a `--targets` flag that controls which targets are started, benchmarked, and reported.

#### Scenario: Default targets include all three
- **WHEN** `bench_services.sh` is invoked without a `--targets` flag
- **THEN** the default target set SHALL be `os,ls,moto`

#### Scenario: Subset of targets can be selected
- **WHEN** `bench_services.sh` is invoked with `--targets os,ls`
- **THEN** only openstack and LocalStack SHALL be started and benchmarked, and moto SHALL be excluded entirely

#### Scenario: openstack target is always required
- **WHEN** `bench_services.sh` is invoked with a `--targets` value that does not include `os`
- **THEN** the harness SHALL exit with an error message indicating that `os` is a required target

#### Scenario: Target flag controls container startup
- **WHEN** a target is not in the `--targets` list
- **THEN** no Docker container, health check, benchmark call, memory collection, or cleanup SHALL occur for that target

### Requirement: Moto image configurability
The benchmark system SHALL allow the moto Docker image to be overridden via a CLI flag.

#### Scenario: Default moto image is used
- **WHEN** `bench_services.sh` is invoked without `--moto-image`
- **THEN** the harness SHALL use `motoserver/moto:latest` as the moto Docker image

#### Scenario: Custom moto image is used
- **WHEN** `bench_services.sh` is invoked with `--moto-image motoserver/moto:5.0.0`
- **THEN** the harness SHALL use `motoserver/moto:5.0.0` as the moto Docker image
