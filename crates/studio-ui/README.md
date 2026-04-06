# openstack-studio-ui

[![CI](https://img.shields.io/github/actions/workflow/status/JesseKoldewijn/openstack/ci.yml?branch=main&label=CI&logo=github)](https://github.com/JesseKoldewijn/openstack/actions/workflows/ci.yml)
[![Docker](https://img.shields.io/github/actions/workflow/status/JesseKoldewijn/openstack/docker.yml?branch=main&label=Docker&logo=docker&logoColor=white)](https://github.com/JesseKoldewijn/openstack/actions/workflows/docker.yml)
[![Stable release](https://img.shields.io/github/v/release/JesseKoldewijn/openstack?sort=semver&label=stable%20release)](https://github.com/JesseKoldewijn/openstack/releases)
[![Beta release](https://img.shields.io/github/v/tag/JesseKoldewijn/openstack?filter=v*-beta.*&sort=semver&label=beta%20release)](https://github.com/JesseKoldewijn/openstack/tags)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](../../LICENSE)

Shared Studio domain crate used by the embedded Studio experience in `openstack`.

This crate contains typed models and runtime helpers used by:

- internal Studio API handlers (`crates/internal-api`)
- gateway Studio transaction/storage/operation integration (`crates/gateway`)
- Studio tests and contract checks

## What lives here

- **Guided manifest parsing/validation** (`guided_manifest`)
- **Operation catalog** (`operation_catalog`)
- **Storage snapshot model** (`storage_inspector`)
- **Transaction log model** (`transaction_log`)
- **Protocol adapters / guided runtime helpers**
- **Typed Studio API DTOs**

## Studio runtime notes

- Manifest protocol classes supported by this crate are:
  - `query`
  - `json_target`
  - `rest_xml`
  - `rest_json`
- Internal API compatibility shims normalize legacy aliases (e.g. `ec2 -> query`) before decoding into `GuidedManifest`.

## Test

```bash
cargo test -p openstack-studio-ui
```

E2E Studio UI tests (Playwright) live under:

- `crates/studio-ui/tests/e2e`

Run:

```bash
cd crates/studio-ui
npx playwright test
```
