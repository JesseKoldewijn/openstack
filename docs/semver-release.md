# Release Flow Status

`main` is currently in a transition period for release automation.

## Current state

- Legacy `release-plz` + RC helper workflows were removed from `main`.
- CI remains the merge safety gate (`Required checks (main target)` / `Required checks (non-main target)`).
- Docker publishing and tag-based binary builds remain active through their dedicated workflows.
- `develop` is promoted to `main` using **fast-forward only** (`git merge --ff-only origin/develop`).

## Why this doc is short

This file previously documented the old `release-plz`-driven flow in detail.
That flow is no longer present on `main`, and the old instructions are intentionally removed to avoid operator confusion.

## Next step

A follow-up PR should reintroduce a single, stable release automation model and then update this document with final, branch-specific behavior.
