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

When creating commits or branches, always branch from and target `develop` unless a maintainer explicitly instructs otherwise.

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
- Do not force-push to `main` without explicit maintainer approval
- Merge strategy for `develop` → `main` is **rebase** (no squash) to keep history reconcilable
