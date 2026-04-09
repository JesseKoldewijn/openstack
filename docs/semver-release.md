# Release Flow

## Branch roles

- `main` publishes the stable container channels:
  - `stable`
  - `latest`
- `develop` publishes the prerelease container channels:
  - `beta`
  - `beta-<short-sha>`

## Versioning

`semantic-release` determines when a new GitHub release/tag should be created:

- `main` → stable releases
- `develop` → `beta` prereleases

Those Git tags are for release/version history.
They are **not** used as an additional Docker publishing path.

## Docker publishing rules

Docker image publication is branch-driven:

- pushes to `main` publish `stable` and `latest`
- pushes to `develop` publish `beta` and `beta-<short-sha>`
- scheduled Docker refreshes rebuild from `main` only
- pull requests build for validation only and do not publish

This keeps the release channels unambiguous and prevents prerelease activity on `develop` from accidentally publishing stable-style container tags.

## Promotion

`develop` is promoted to `main` using **fast-forward only**:

```bash
git checkout main
git fetch origin
git merge --ff-only origin/develop
```
