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

This repo uses Conventional Commit semantics for automated SemVer versioning.
Because PRs to `develop` are squash-merged, **PR title is the effective release signal**.

Use lowercase imperative prefix commits / PR titles:

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

SemVer mapping:
- feat -> minor
- fix/perf -> patch
- breaking (`!` or `BREAKING CHANGE`) -> major

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
- Merge strategy depends on target branch:
  - **PRs → `develop`**: use **Squash and merge** — one clean commit per PR on `develop`.
  - **`develop` → `main`**: use **Create a merge commit** — never squash, never rebase.
    GitHub's "Rebase and merge" button rewrites commit SHAs, causing false conflicts on the next
    sync. Always use "Create a merge commit" when merging `develop` into `main`.

## Release automation context (for agents)

- Release PR automation: `.github/workflows/release-plz.yml` (develop)
- Release execution: `.github/workflows/release.yml` (main)
- Docker channels: `.github/workflows/docker.yml`
  - main = stable (`latest` + semver tags)
  - develop = RC (`rc`, `rc-<short-sha>`)
- Tag build pipeline: `.github/workflows/cross-compile.yml` (`v*.*.*` tags)

When preparing PRs, keep titles semantic and avoid vague titles like "updates".
Refer to `docs/semver-release.md` for full details.
