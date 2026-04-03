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

Use concise, lowercase imperative commit messages:

```
feat: add SQS dead-letter queue support
fix: resolve S3 multipart upload race condition
chore: update dependencies
```

Prefixes: `feat`, `fix`, `chore`, `refactor`, `test`, `docs`, `perf`, `ci`.

---

## Code style

- Run `cargo fmt` before committing.
- Run `cargo clippy` and address any warnings.
- Follow existing patterns in the crate you are modifying.

---

## main ← develop merges

Merges from `develop` into `main` are performed by maintainers. The merge strategy is **rebase** (no squash) to preserve commit history and keep branch histories reconcilable.

---

## CI requirements

**All CI checks must pass before a PR may be merged** into either `develop` or `main`. The CI workflow exposes two aggregate gate jobs that enforce this:

| Gate | Applies to |
|------|------------|
| `Required checks (non-main target)` | PRs targeting `develop` |
| `Required checks (main target)` | PRs targeting `main` |

These gates cover: formatting (`cargo fmt`), linting (`cargo clippy`), tests, release build verification, harness coverage, Studio UI/asset/coverage checks, parity (core and all-services smoke), and benchmark.

Do not attempt to merge a PR while any required check is pending or failing.
