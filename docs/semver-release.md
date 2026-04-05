# Automated SemVer & Release Pipeline

This repository uses **semantic-release** for automated versioning and GitHub releases.

## Release channels

- `main` → **stable/latest** releases
- `develop` → **beta/canary** prereleases
- Pull requests → **no release tags** (CI validation only)

## Workflows

- **Semantic release**: `.github/workflows/semantic-release.yml`
  - Trigger: push to `main` and `develop` (or manual dispatch)
  - Behavior:
    - `main` publishes stable release tags (`vX.Y.Z`)
    - `develop` publishes beta prerelease tags (`vX.Y.Z-beta.N`)

- **Docker publishing**: `.github/workflows/docker.yml`
  - Trigger: pushes to `main`, `develop`, and tag events
  - Output:
    - `main`: `stable`, `latest`
    - `develop`: `beta`, `beta-<short-sha>`
    - semver tags: published from git tag events
  - Pull requests build images but do not publish tags.

- **Badge updater**: `.github/workflows/release-badges.yml`
  - Maintains Shields endpoint JSON files on `gh-pages`:
    - `badges/stable.json`
    - `badges/beta.json`

- **Cleanup**: `.github/workflows/cleanup-ghcr.yml`
  - Purges untagged/ephemeral GHCR package versions.

## Commit conventions (SemVer signals)

- `feat:` → minor
- `fix:` / `perf:` → patch
- `!` or `BREAKING CHANGE:` → major

Because PRs are squash-merged, **PR title is the effective release signal**.

## Token requirements

Set repository secret `RELEASE_PLZ_TOKEN` (PAT with `repo` scope) for release workflows.

This token is used by semantic-release to create tags/releases and avoids GitHub Actions token recursion restrictions.

## Runtime version visibility

CI injects build metadata into binaries/images via:

- `OPENSTACK_BUILD_TAG`
- `OPENSTACK_BUILD_SHA`

Exposed in:

- `openstack --version`
- `/_localstack/health`
- `/_localstack/info`
- `/_localstack/diagnose`
