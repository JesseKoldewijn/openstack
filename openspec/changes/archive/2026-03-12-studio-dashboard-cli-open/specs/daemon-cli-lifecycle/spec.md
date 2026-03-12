## MODIFIED Requirements

### Requirement: Lifecycle control commands
The CLI SHALL support `status`, `stop`, and `restart` lifecycle commands for managed daemon instances and SHALL use graceful shutdown semantics before forceful termination.

The CLI SHALL also support Studio-open commands that integrate with daemon/runtime availability checks and browser-launch behavior.

#### Scenario: Status reports running instance
- **WHEN** a managed daemon is healthy and `openstack status` is invoked
- **THEN** the CLI SHALL report running state and key runtime metadata including endpoint and process identity

#### Scenario: Stop performs graceful shutdown
- **WHEN** a user runs `openstack stop` for a running managed daemon
- **THEN** the CLI SHALL initiate graceful shutdown and confirm termination once complete

#### Scenario: Restart cycles instance safely
- **WHEN** a user runs `openstack restart`
- **THEN** the CLI SHALL stop the existing managed daemon and start a new managed daemon with equivalent configuration

#### Scenario: Studio command opens URL
- **WHEN** a user runs `openstack studio`
- **THEN** the CLI SHALL resolve the Studio URL and attempt to open it in the default browser

#### Scenario: Studio URL fallback is actionable
- **WHEN** `openstack studio` cannot launch a browser
- **THEN** the CLI SHALL print the Studio URL and actionable guidance without crashing

#### Scenario: Studio print-url mode is deterministic
- **WHEN** a user runs `openstack studio --print-url`
- **THEN** the CLI SHALL print the resolved Studio URL and SHALL skip browser-launch attempts
