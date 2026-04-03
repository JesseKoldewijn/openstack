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
