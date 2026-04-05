# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.5](https://github.com/JesseKoldewijn/openstack/compare/v0.1.4...v0.1.5) - 2026-04-05

### Added

- *(studio)* full tab-based service explorer with SigV4, polling, and multi-step guided flows ([#15](https://github.com/JesseKoldewijn/openstack/pull/15))
- ECR and EventBridge lifecycle parity scenarios and expanded benchmarks ([#9](https://github.com/JesseKoldewijn/openstack/pull/9))
- finalize native HTTP parity readiness ([#6](https://github.com/JesseKoldewijn/openstack/pull/6))
- implement streaming IO benchmark overhaul and sync specs
- isolate benchmark lanes from aws cli overhead
- additional performance enhancement changes

### Fixed

- resolve ci lint failures and parity runtime permissions
- harden benchmark container health diagnostics
- validate AWS_CLI_PATH before benchmark command execution
- satisfy clippy default field assignment lint

### Other

- *(release)* release-plz
- *(release)* release-plz ([#44](https://github.com/JesseKoldewijn/openstack/pull/44))
- *(release)* release-plz
- *(release)* release-plz
- *(s3)* reduce blocking-pool pressure and lock hold time under concurrency ([#10](https://github.com/JesseKoldewijn/openstack/pull/10))
- Merge pull request #4 from JesseKoldewijn/feat/perf-improvements-io
- use deterministic runtime image for parity benchmarks
- apply rustfmt ordering and wrapping cleanups
- surface invalid benchmark signal details in strict gate failures
- harden benchmark signal quality and core lane gating
- removal/archival of harness plan
- enforce branch-aware smoke lanes with PR result tables
- project initialization

## [0.1.4](https://github.com/JesseKoldewijn/openstack/compare/v0.1.3...v0.1.4) - 2026-04-05

### Added

- *(studio)* full tab-based service explorer with SigV4, polling, and multi-step guided flows ([#15](https://github.com/JesseKoldewijn/openstack/pull/15))
- ECR and EventBridge lifecycle parity scenarios and expanded benchmarks ([#9](https://github.com/JesseKoldewijn/openstack/pull/9))
- finalize native HTTP parity readiness ([#6](https://github.com/JesseKoldewijn/openstack/pull/6))
- implement streaming IO benchmark overhaul and sync specs
- isolate benchmark lanes from aws cli overhead
- additional performance enhancement changes

### Fixed

- resolve ci lint failures and parity runtime permissions
- harden benchmark container health diagnostics
- validate AWS_CLI_PATH before benchmark command execution
- satisfy clippy default field assignment lint

### Other

- *(release)* release-plz ([#44](https://github.com/JesseKoldewijn/openstack/pull/44))
- *(release)* release-plz
- *(release)* release-plz
- *(s3)* reduce blocking-pool pressure and lock hold time under concurrency ([#10](https://github.com/JesseKoldewijn/openstack/pull/10))
- Merge pull request #4 from JesseKoldewijn/feat/perf-improvements-io
- use deterministic runtime image for parity benchmarks
- apply rustfmt ordering and wrapping cleanups
- surface invalid benchmark signal details in strict gate failures
- harden benchmark signal quality and core lane gating
- removal/archival of harness plan
- enforce branch-aware smoke lanes with PR result tables
- project initialization

## [0.1.3](https://github.com/JesseKoldewijn/openstack/compare/v0.1.2...v0.1.3) - 2026-04-05

### Added

- *(studio)* full tab-based service explorer with SigV4, polling, and multi-step guided flows ([#15](https://github.com/JesseKoldewijn/openstack/pull/15))
- ECR and EventBridge lifecycle parity scenarios and expanded benchmarks ([#9](https://github.com/JesseKoldewijn/openstack/pull/9))
- finalize native HTTP parity readiness ([#6](https://github.com/JesseKoldewijn/openstack/pull/6))
- implement streaming IO benchmark overhaul and sync specs
- isolate benchmark lanes from aws cli overhead
- additional performance enhancement changes

### Fixed

- resolve ci lint failures and parity runtime permissions
- harden benchmark container health diagnostics
- validate AWS_CLI_PATH before benchmark command execution
- satisfy clippy default field assignment lint

### Other

- *(release)* release-plz
- *(release)* release-plz
- *(s3)* reduce blocking-pool pressure and lock hold time under concurrency ([#10](https://github.com/JesseKoldewijn/openstack/pull/10))
- Merge pull request #4 from JesseKoldewijn/feat/perf-improvements-io
- use deterministic runtime image for parity benchmarks
- apply rustfmt ordering and wrapping cleanups
- surface invalid benchmark signal details in strict gate failures
- harden benchmark signal quality and core lane gating
- removal/archival of harness plan
- enforce branch-aware smoke lanes with PR result tables
- project initialization

## [0.1.2](https://github.com/JesseKoldewijn/openstack/compare/v0.1.1...v0.1.2) - 2026-04-05

### Added

- *(studio)* full tab-based service explorer with SigV4, polling, and multi-step guided flows ([#15](https://github.com/JesseKoldewijn/openstack/pull/15))
- ECR and EventBridge lifecycle parity scenarios and expanded benchmarks ([#9](https://github.com/JesseKoldewijn/openstack/pull/9))
- finalize native HTTP parity readiness ([#6](https://github.com/JesseKoldewijn/openstack/pull/6))
- implement streaming IO benchmark overhaul and sync specs
- isolate benchmark lanes from aws cli overhead
- additional performance enhancement changes

### Fixed

- resolve ci lint failures and parity runtime permissions
- harden benchmark container health diagnostics
- validate AWS_CLI_PATH before benchmark command execution
- satisfy clippy default field assignment lint

### Other

- *(release)* release-plz
- *(s3)* reduce blocking-pool pressure and lock hold time under concurrency ([#10](https://github.com/JesseKoldewijn/openstack/pull/10))
- Merge pull request #4 from JesseKoldewijn/feat/perf-improvements-io
- use deterministic runtime image for parity benchmarks
- apply rustfmt ordering and wrapping cleanups
- surface invalid benchmark signal details in strict gate failures
- harden benchmark signal quality and core lane gating
- removal/archival of harness plan
- enforce branch-aware smoke lanes with PR result tables
- project initialization

## [0.1.1](https://github.com/JesseKoldewijn/openstack/compare/v0.1.0...v0.1.1) - 2026-04-05

### Added

- *(studio)* full tab-based service explorer with SigV4, polling, and multi-step guided flows ([#15](https://github.com/JesseKoldewijn/openstack/pull/15))
- ECR and EventBridge lifecycle parity scenarios and expanded benchmarks ([#9](https://github.com/JesseKoldewijn/openstack/pull/9))
- finalize native HTTP parity readiness ([#6](https://github.com/JesseKoldewijn/openstack/pull/6))
- implement streaming IO benchmark overhaul and sync specs
- isolate benchmark lanes from aws cli overhead
- additional performance enhancement changes

### Fixed

- resolve ci lint failures and parity runtime permissions
- harden benchmark container health diagnostics
- validate AWS_CLI_PATH before benchmark command execution
- satisfy clippy default field assignment lint

### Other

- *(s3)* reduce blocking-pool pressure and lock hold time under concurrency ([#10](https://github.com/JesseKoldewijn/openstack/pull/10))
- Merge pull request #4 from JesseKoldewijn/feat/perf-improvements-io
- use deterministic runtime image for parity benchmarks
- apply rustfmt ordering and wrapping cleanups
- surface invalid benchmark signal details in strict gate failures
- harden benchmark signal quality and core lane gating
- removal/archival of harness plan
- enforce branch-aware smoke lanes with PR result tables
- project initialization
