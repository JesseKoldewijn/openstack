# Automated SemVer & Release Pipeline

This repository uses **release-plz** for automated versioning and release automation.

## Why

- Keep versions/changelogs consistent
- Use commit intent to determine SemVer bump
- Minimize manual release mistakes
- Make release behavior predictable for humans and coding agents

## Commit/PR title convention

SemVer decisions are based on Conventional Commit style.

- `feat: ...` → **minor**
- `fix: ...`, `perf: ...` → **patch**
- `feat!: ...` or `BREAKING CHANGE:` footer → **major**

Recommended PR titles (important because PRs are squash-merged to `develop`):

```text
feat(gateway): support virtual-hosted-style s3 paths
fix(studio): prevent duplicate transaction rows
feat!: remove deprecated localstack_host fallback
```

## Branch integration

- Feature/fix work lands in `develop` via PR.
- `release-plz` creates/updates a release PR from `develop` changes.
- `develop` is promoted to `main` via merge commit.
- Release automation on `main` creates stable tags/releases.

Release channels:
- `main` = **stable** channel
- `develop` = **RC** channel (container tags `rc` + `rc-<short-sha>`)

## Workflows

- **Release PR automation**: `.github/workflows/release-plz.yml`
  - Trigger: push to `develop`
  - Command: `release-plz release-pr`
  - Output: release PR with version/changelog updates

- **Release automation**: `.github/workflows/release.yml`
  - Trigger: push to `main` (or manual)
  - Command: `release-plz release`
  - Output: git tag(s) + GitHub release(s)

- **Docker publishing**: `.github/workflows/docker.yml`
  - Trigger: `main`, `develop`, and semver tags `v*.*.*`
  - Output:
    - `main`: `latest` (+ semver tags on tag events)
    - `develop`: `rc`, `rc-<short-sha>`

- **Tag-based builds**: `.github/workflows/cross-compile.yml`
  - Trigger: `v*.*.*` tags
  - Output: release binaries for amd64/arm64

## Configuration

- `release-plz` config lives in `.release-plz.toml`.
- Current settings:
  - changelog updates enabled
  - dependency updates enabled
  - git release enabled
  - git tag format `v{{ version }}`
  - crates.io publish disabled

## Token recommendation

Set `RELEASE_PLZ_TOKEN` (PAT with `repo` scope) for release workflows.

Reason: tags created with default `GITHUB_TOKEN` may not trigger downstream workflows in all org/repo configurations.
Using a PAT makes tag-triggered workflows (like `cross-compile`) reliable.
