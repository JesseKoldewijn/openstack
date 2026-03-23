# Guided Manifest Inventory

This directory stores one guided manifest per supported service.

- Naming: `<service>.guided.json`
- Required: at least one `L1` flow with assertions and cleanup steps
- Schema version: `1.2`

Validation commands:

```bash
python3 scripts/studio_guided_coverage_gate.py
python3 scripts/studio_coverage_report.py
cargo test -p openstack-studio-ui guided_manifest::tests -- --nocapture
```
