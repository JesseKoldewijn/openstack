## 1. Daemon CLI Foundation

- [x] 1.1 Add CLI subcommand model in `crates/openstack` for `start`, `stop`, `status`, `restart`, and `logs` while preserving existing foreground default behavior.
- [x] 1.2 Implement `start --daemon` process launch flow with detached execution, startup validation, and explicit success/failure exit codes.
- [x] 1.3 Implement managed instance state tracking (pid/lock/metadata files) with stale-state recovery logic.
- [x] 1.4 Implement `status` command with combined process check + health endpoint verification and degraded-state reporting.
- [x] 1.5 Implement `stop` command with graceful shutdown first, bounded wait, and forceful fallback behavior.
- [x] 1.6 Implement `restart` as stop/start orchestration with failure-safe rollback semantics when start fails.
- [x] 1.7 Implement `logs` and `logs --follow` command behavior using managed daemon log sink.
- [x] 1.8 Add CLI-focused unit and integration tests for lifecycle command behavior, duplicate start prevention, stale metadata handling, and non-zero failure exit paths.

## 2. Gateway and Internal Route Integration

- [x] 2.1 Refactor gateway internal route handling so Studio routes (`/_localstack/studio/*`) and Studio API routes (`/_localstack/studio-api/*`) are resolved before AWS service inference.
- [x] 2.2 Add static asset serving and SPA fallback behavior for Studio entry and nested client-side routes.
- [x] 2.3 Introduce compressed/cached asset response behavior for Studio production artifacts.
- [x] 2.4 Integrate or unify existing `internal-api` router usage to reduce duplicate endpoint handling logic.
- [x] 2.5 Extend internal API responses (`health`, `info`, `plugins`) with daemon and Studio metadata fields required by CLI and frontend.
- [x] 2.6 Add Studio API endpoints for service catalog metadata and interaction schema discovery.
- [x] 2.7 Add gateway/internal API integration tests for route precedence, AWS route non-regression, Studio fallback routing, and Studio API response contracts.

## 3. Studio SPA Project Setup

- [x] 3.1 Add a new frontend workspace/crate for Leptos SPA with reproducible build scripts and CI-friendly install/build steps.
- [x] 3.2 Configure Tailwind CSS v4 with CSS-based configuration and design token variables for shared theming.
- [x] 3.3 Implement global application shell (navigation, layout, responsive behavior, keyboard focus baseline).
- [x] 3.4 Implement light/dark theme switcher with persistent user preference and startup theme hydration.
- [x] 3.5 Implement API client layer for `/_localstack/studio-api/*` and raw interaction execution with typed request/response models.
- [x] 3.6 Add frontend unit tests for state stores, theme persistence, request serialization, and response parsing behavior.

## 4. Studio Core Features

- [x] 4.1 Build service catalog screen rendering all enabled services with support tier badges (`guided`, `raw`, `coming-soon`).
- [x] 4.2 Build raw interaction console with editable method/path/query/headers/body inputs and full response envelope rendering.
- [x] 4.3 Build interaction history panel with timestamped entries, filtering, detail inspection, and request replay prefill.
- [x] 4.4 Build guided workflow framework that can define multi-step flows backed by real openstack API calls.
- [x] 4.5 Implement initial guided workflows for representative services (at minimum S3 and SQS) using real endpoints and visible post-action state.
- [x] 4.6 Add component/integration tests for catalog rendering, console execution states, history replay, and guided flow UI progression.

## 5. Studio-to-Backend E2E Validation

- [x] 5.1 Add browser E2E test harness that starts/stops openstack daemon deterministically for test sessions.
- [x] 5.2 Add E2E scenario for Studio boot and theme persistence (toggle theme, reload, verify persisted mode).
- [x] 5.3 Add E2E scenario for raw request path (issue request, validate response envelope, verify backend side effect).
- [x] 5.4 Add E2E scenario for guided S3 flow (create bucket, upload object, verify read-back success).
- [x] 5.5 Add E2E scenario for guided SQS flow (create queue, send message, verify receive/delete behavior).
- [x] 5.6 Add E2E scenario for daemon lifecycle from CLI (`start --daemon`, `status`, `restart`, `stop`) with health-aware assertions.
- [x] 5.7 Add deterministic fixture setup/teardown and resource namespacing to prevent cross-test contamination.
- [x] 5.8 Add CI workflow gates requiring frontend unit/component tests and Studio E2E suites for Studio-affecting changes.

## 6. Coverage Expansion and Quality Gates

- [x] 6.1 Define and generate a Studio service coverage report in CI showing guided/raw/coming-soon coverage by service.
- [x] 6.2 Add contract tests validating Studio API schema compatibility between backend responses and frontend expectations.
- [x] 6.3 Add performance checks for Studio asset size and initial render budget to prevent regressions.
- [x] 6.4 Add security checks for Studio/internal routes (method allow-lists, payload bounds, and safe error exposure).
- [x] 6.5 Add documentation for daemon workflows, Studio usage, troubleshooting, and contribution/testing instructions.
- [x] 6.6 Add release-readiness checklist to verify non-regression across existing parity/benchmark suites with Studio and daemon features enabled.
