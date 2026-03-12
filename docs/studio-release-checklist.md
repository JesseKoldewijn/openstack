# Studio Release Readiness Checklist

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] `cargo test -p openstack-studio-ui`
- [ ] `cargo test -p openstack-integration-tests --test studio_e2e_tests`
- [ ] `cargo test -p openstack-gateway --tests`
- [ ] `cargo test -p openstack-internal-api --tests`
- [ ] `python3 scripts/studio_coverage_report.py`
- [ ] `python3 scripts/studio_guided_coverage_gate.py`
- [ ] `scripts/check_studio_asset_budget.sh`
- [ ] Parity core and benchmark smoke workflows still pass in CI
- [ ] Guided manifest docs updated (`authoring`, `template`, `troubleshooting`, `change-management`)
