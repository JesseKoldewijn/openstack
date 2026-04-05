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
- `main` = **stable** channel (container tags `stable`, `latest`)
- `develop` = **RC** channel (container tags `rc`, `rc-<short-sha>`)
- `pull_request` = **RC preview** tags (same-repo PRs)
  - immutable: `v<base-rc>.pr-<number>`
  - mutable per-PR pointer: `pr-<number>` (updated on each PR build)
  - PR label: `@version:<immutable-tag>` (replaced/updated on each PR push)
  - example immutable tag: `v1.0.0-rc-2.pr-33`
  - stale preview versions are removed during the workflow cleanup cycle

## Workflows

- **Release PR automation**: `.github/workflows/release-plz.yml`
  - Trigger: push to `develop`
  - Command: `release-plz release-pr`
  - Output: release PR with version/changelog updates
  - Guardrails:
    - auto-creates required PR labels (`release`, `semver`) before running
    - cleans up stale `release-plz-*` branches that are not linked to open PRs

- **Develop RC tag automation**: `.github/workflows/develop-rc-tag.yml`
  - Trigger: push to `develop`
  - Output: next RC tag on current develop head (`v<stable>-rc-<n>`)

- **PR version label automation**: `.github/workflows/pr-version-label.yml`
  - Trigger: PR opened/synchronized/reopened
  - Output: updates PR label `@version:v<base-rc>.pr-<number>`

- **Release automation**: `.github/workflows/release.yml`
  - Trigger: push to `main` (or manual)
  - Guard: on push events, runs only when the head commit message indicates a merged release-plz PR (`chore(release): release-plz`)
  - Command: `release-plz release`
  - Output: git tag(s) + GitHub release(s)

- **Docker publishing**: `.github/workflows/docker.yml`
  - Trigger: `main`, `develop`, pull requests, and semver/rc tags `v*.*.*`
  - Output:
    - `main`: `stable`, `latest` (+ semver tags on tag events)
    - `develop`: `rc`, `rc-<short-sha>`
    - `pull_request`: `v<base-rc>.pr-<number>` + `pr-<number>` (same-repo PRs)
  - Passes `OPENSTACK_BUILD_TAG` / `OPENSTACK_BUILD_SHA` build args into Docker builds so binary version output and API metadata match image channel/tag identity.

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

## Runtime version visibility

With build metadata configured by CI/workflows:

- `openstack --version` prints semantic version plus build identity
- `/_localstack/health`, `/_localstack/info`, and `/_localstack/diagnose` include:
  - `version` (Cargo package version)
  - `version_display` (human-readable version including build metadata)
  - `build` object (`tag`, `sha`)

## Token recommendation

Set `RELEASE_PLZ_TOKEN` (PAT with `repo` scope) for release workflows.

Reason: tags created with default `GITHUB_TOKEN` may not trigger downstream workflows in all org/repo configurations.
Using a PAT makes tag-triggered workflows (like `cross-compile`) reliable.
