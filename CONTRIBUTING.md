# Contributing to openstack

Thank you for your interest in contributing. Please read this guide before opening issues or pull requests.

---

## Branch model

This project follows a two-branch model:

| Branch | Purpose |
|--------|---------|
| `develop` | Active development. All features, fixes, and improvements land here first. |
| `main` | Stable release branch. Only receives merges from `develop` once changes are verified. |

**All pull requests must target `develop`.**
Direct PRs to `main` will not be accepted unless they are hotfixes explicitly approved by a maintainer.

**Never commit directly to `develop` or `main`.**
All work — features, fixes, refactors, documentation — must arrive via a feature-branch PR. Direct pushes to `develop` are not permitted.

---

## Workflow

1. Fork the repository.
2. Create a feature branch off `develop`:
   ```
   git checkout develop
   git checkout -b feat/your-feature-name
   ```
3. Make your changes, write tests where appropriate.
4. Ensure the project builds and checks pass locally:
   ```
   cargo check
   cargo test
   ```
5. Open a pull request targeting **`develop`**.
6. Once reviewed and merged into `develop`, the change will eventually be promoted to `main` by a maintainer.

> `develop` is a shared integration branch. Committing to it directly bypasses review and risks destabilising it for other contributors.

---

## Commit style

This repo uses Conventional Commit semantics for automated SemVer versioning.
Because PRs to `develop` are squash-merged, **your PR title should follow the same format**.

Use concise, lowercase imperative commit messages / PR titles:

```
feat: add SQS dead-letter queue support
fix: resolve S3 multipart upload race condition
chore: update dependencies
```

Prefixes: `feat`, `fix`, `chore`, `refactor`, `test`, `docs`, `perf`, `ci`.

SemVer mapping:
- `feat` → minor
- `fix` / `perf` → patch
- `!` or `BREAKING CHANGE` footer → major

---

## Code style

- Run `cargo fmt` before committing.
- Run `cargo clippy` and address any warnings.
- Follow existing patterns in the crate you are modifying.

---

## Merge strategy

The merge strategy depends on the target branch:

| PR target | Strategy | Why |
|-----------|----------|-----|
| `develop` | **Squash and merge** | Keeps `develop` history clean — one commit per PR. |
| `main` | **Fast-forward only** | Guarantees `develop` is always ahead of or equal to `main`. |

### `develop` → `main` promotion (fast-forward only)

Do **not** use GitHub PR merge buttons for `main` promotion.

Use:

```bash
git fetch origin
git checkout main
git reset --hard origin/main
git merge --ff-only origin/develop
git push origin main
```

If `git merge --ff-only` fails, `main` has drifted and must be reconciled before promotion.

---

## CI requirements

**All CI checks must pass before a PR may be merged** into either `develop` or `main`. The CI workflow exposes two aggregate gate jobs that enforce this:

| Gate | Applies to |
|------|------------|
| `Required checks (non-main target)` | PRs targeting `develop` |
| `Required checks (main target)` | PRs targeting `main` |

These gates cover: formatting (`cargo fmt`), linting (`cargo clippy`), tests, release build verification, harness coverage, Studio UI/asset/coverage checks, parity (core and all-services smoke), and benchmark.

Do not attempt to merge a PR while any required check is pending or failing.

## Release pipeline

Release automation is currently being reworked on `main`.

- legacy `release-plz`/RC helper workflows were removed
- CI and Docker pipelines remain active and required
- tag-driven binary builds run on `v*.*.*` tags

See [`docs/semver-release.md`](docs/semver-release.md) for current release-flow status.
