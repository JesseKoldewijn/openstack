# Studio Guided Flow Change Management

## Versioning policy

- Schema follows `major.minor`.
- `minor` changes are additive and backward-compatible.
- `major` changes are breaking and require migration guidance.

## Compatibility expectations

- Runtime supporting `1.2` accepts manifests from `1.0` to `1.2`.
- Runtime rejects manifests with higher minor (`1.3`) or different major (`2.x`).

## Change process

1. Update schema and validation logic.
2. Add/adjust tests for compatibility behavior.
3. Update all affected manifests.
4. Update docs:
   - authoring guide
   - contribution template
   - troubleshooting
   - release checklist

## Release expectations

- CI manifest coverage gate passes.
- Schema/semantic validation passes.
- Representative protocol E2E tests pass.
