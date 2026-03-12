## Purpose

Define CLI lifecycle behavior for managing openstack as a daemonized process.

## ADDED Requirements

### Requirement: Daemonized startup command
The openstack CLI SHALL support starting openstack as a background daemon process and SHALL expose deterministic startup success/failure feedback to the caller.

#### Scenario: Daemon start succeeds
- **WHEN** a user runs `openstack start --daemon` while no managed instance is running
- **THEN** the CLI SHALL launch openstack in the background, report success, and provide enough metadata to locate the running instance

#### Scenario: Daemon start fails with actionable message
- **WHEN** daemon startup fails due to configuration, port binding, or runtime initialization error
- **THEN** the CLI SHALL return a non-zero exit code and a clear diagnostic message

### Requirement: Single-instance process ownership
The daemon lifecycle SHALL enforce single-instance ownership for a managed environment and SHALL prevent duplicate managed instances from being started concurrently.

#### Scenario: Duplicate start is prevented
- **WHEN** a managed daemon instance is already running and a user runs `openstack start --daemon` again
- **THEN** the CLI SHALL not launch a second managed daemon and SHALL report the existing instance state

#### Scenario: Stale instance metadata is recovered
- **WHEN** daemon metadata exists but the tracked process is not alive
- **THEN** the CLI SHALL recover by cleaning stale metadata and allowing a fresh daemon start

### Requirement: Lifecycle control commands
The CLI SHALL support `status`, `stop`, and `restart` lifecycle commands for managed daemon instances and SHALL use graceful shutdown semantics before forceful termination.

#### Scenario: Status reports running instance
- **WHEN** a managed daemon is healthy and `openstack status` is invoked
- **THEN** the CLI SHALL report running state and key runtime metadata including endpoint and process identity

#### Scenario: Stop performs graceful shutdown
- **WHEN** a user runs `openstack stop` for a running managed daemon
- **THEN** the CLI SHALL initiate graceful shutdown and confirm termination once complete

#### Scenario: Restart cycles instance safely
- **WHEN** a user runs `openstack restart`
- **THEN** the CLI SHALL stop the existing managed daemon and start a new managed daemon with equivalent configuration

### Requirement: Health-aware status verification
Daemon status SHALL combine process-level checks with service health checks to avoid false-positive running states.

#### Scenario: Process exists but service unhealthy
- **WHEN** the daemon process exists but health endpoint checks fail
- **THEN** `openstack status` SHALL report a degraded or unhealthy state rather than fully healthy running

#### Scenario: Process missing and health unavailable
- **WHEN** no daemon process exists and health endpoint is unreachable
- **THEN** `openstack status` SHALL report not-running

### Requirement: Daemon log access
The CLI SHALL provide log access for managed daemon instances, including tail and follow behaviors.

#### Scenario: Log tail returns recent entries
- **WHEN** a user runs `openstack logs`
- **THEN** the CLI SHALL print recent daemon log lines from the managed log sink

#### Scenario: Log follow streams entries
- **WHEN** a user runs `openstack logs --follow`
- **THEN** the CLI SHALL stream new daemon log entries until interrupted
