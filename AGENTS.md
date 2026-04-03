# AGENTS.md

Conventions and rules for AI agents (Copilot, Claude, OpenCode, etc.) working in this repository.

---

## Branch model

This project uses a two-branch model. Agents must respect it:

| Branch | Role |
|--------|------|
| `develop` | Integration branch. All new work lands here first. |
| `main` | Stable branch. Only receives merges from `develop`. |

**Features, fixes, refactors, and all other changes must be proposed against `develop`.**
`main` is only updated by merging from `develop` — never directly.

**Never commit directly to `develop` or `main`.**
All work must go through a feature branch and be merged into `develop` via a pull request. Direct commits to `develop` bypass review and are not permitted.

When creating commits or branches, always branch from `develop` and open a PR targeting `develop`. Never push commits directly to `develop` or `main` unless a maintainer explicitly instructs otherwise.

---

## Commit style

Use lowercase imperative prefix commits:

```
feat: <description>
fix: <description>
chore: <description>
refactor: <description>
test: <description>
docs: <description>
perf: <description>
ci: <description>
```

---

## Code conventions

- **Language:** Rust (2024 edition)
- **Formatter:** `cargo fmt` — run before committing
- **Linter:** `cargo clippy` — address all warnings
- **Check before proposing:** always run `cargo check` to verify the workspace compiles

---

## Pull requests

- All PRs must target `develop`
- **All CI checks must pass before a PR may be merged.** The CI workflow enforces this via two aggregate gate jobs:
  - `Required checks (non-main target)` — must pass for PRs targeting `develop`
  - `Required checks (main target)` — must pass for PRs targeting `main`
  These gates cover: formatting, clippy, tests, build artifact verification, harness coverage, studio checks, parity core, parity all-services smoke, and benchmark.
- Do not force-push to `main` without explicit maintainer approval
- Merge strategy for `develop` → `main` is **merge commit only** — never squash, never rebase.
  GitHub's "Rebase and merge" button rewrites commit SHAs, causing false conflicts on the next
  sync. Always use "Create a merge commit" when merging `develop` into `main`.
