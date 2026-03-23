# Per-Service Manifest Review Checklist

- [ ] File path matches `manifests/guided/<service>.guided.json`
- [ ] `schemaVersion` is `1.2`
- [ ] Protocol class matches service wire protocol
- [ ] Includes at least one `L1` flow
- [ ] Includes assertions per flow
- [ ] Includes cleanup per flow
- [ ] Uses approved interpolation expressions only
- [ ] Validated via local commands
