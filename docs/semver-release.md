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

GitHub release notes are enriched automatically with a GHCR package link and the relevant pull tags for the release channel:

- stable releases link to `stable` and `latest`
- beta releases link to `beta` and `beta-<short-sha>`

## Docker publishing rules

Docker image publication is branch-driven:

- pushes to `main` publish `stable` and `latest`
- pushes to `develop` publish `beta` and `beta-<short-sha>`
- scheduled Docker refreshes rebuild from `main` only
- pull requests build for validation only and do not publish

This keeps the release channels unambiguous and prevents prerelease activity on `develop` from accidentally publishing stable-style container tags.

## Promotion

`develop` is promoted to `main` using **fast-forward only**.

### Recommended path

Do **not** open a PR from `develop` to `main`.
Treat this as a promotion step, not a merge-review step.

Use the manual GitHub Actions workflow **Promote develop to main** from the `develop` branch.
That workflow:

- fetches `origin/main` and `origin/develop`
- verifies `main` is an ancestor of `develop`
- runs `git merge --ff-only origin/develop`
- pushes the updated `main` ref only if the promotion is a true fast-forward

This keeps `main` as a clean promoted snapshot of `develop` without merge commits, squash commits, or rebased SHAs.

### CLI fallback

```bash
git checkout main
git fetch origin
git merge --ff-only origin/develop
git push origin main
```

### GitHub branch settings

For `main`, combine the workflow with branch protection / rulesets that:

- require linear history
- require status checks
- restrict who can push to `main`
- block force pushes and deletions

Keep feature-branch merges flexible on `develop`, but treat `develop` -> `main` as a dedicated promotion step rather than a normal PR merge-button flow.

## Team rule

- open PRs into `develop`
- do **not** open PRs from `develop` to `main`
- promote `develop` to `main` only via the fast-forward workflow (or the equivalent CLI ff-only command)
