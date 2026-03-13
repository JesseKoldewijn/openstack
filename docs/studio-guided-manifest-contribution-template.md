# Studio Guided Manifest Contribution Template

Use this template when adding/updating a guided manifest.

## Service

- Service name:
- Protocol class (`query|json_target|rest_xml|rest_json`):
- Manifest file path (`manifests/guided/<service>.guided.json`):

## L1 flow checklist

- [ ] Has at least one L1 flow
- [ ] Includes create/use/verify semantics
- [ ] Includes cleanup semantics
- [ ] Includes at least one assertion per flow
- [ ] Uses only approved interpolation sources

## Validation checklist

- [ ] `cargo test -p openstack-studio-ui guided_manifest::tests -- --nocapture`
- [ ] `python3 scripts/studio_guided_coverage_gate.py`
- [ ] `python3 scripts/studio_coverage_report.py`

## Notes

- Any protocol-specific nuances:
- Any expected temporary limitations:
