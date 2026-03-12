## ADDED Requirements

### Requirement: Studio browser-open command
The openstack CLI SHALL provide a Studio command that resolves the Studio URL and attempts to open it in the user default browser.

#### Scenario: Studio command opens browser on supported platform
- **WHEN** a user runs `openstack studio`
- **THEN** the CLI SHALL resolve the Studio URL and invoke the platform-appropriate browser opener command

#### Scenario: Studio command prints URL for user confirmation
- **WHEN** `openstack studio` runs
- **THEN** the CLI SHALL print the Studio URL regardless of opener success to support copy/paste and headless usage

### Requirement: Headless-safe fallback behavior
The Studio CLI command SHALL degrade gracefully when no browser opener is available and SHALL not crash.

#### Scenario: Browser opener unavailable
- **WHEN** the platform opener command is missing or fails
- **THEN** the CLI SHALL return an actionable message and the Studio URL without panicking

#### Scenario: Explicit URL-only mode
- **WHEN** a user runs `openstack studio --print-url`
- **THEN** the CLI SHALL print the resolved URL and SHALL NOT attempt to launch a browser

### Requirement: Daemon-awareness for Studio open
The Studio CLI command SHALL account for daemon lifecycle state and provide deterministic behavior when the runtime is unavailable.

#### Scenario: Daemon running and Studio reachable
- **WHEN** `openstack studio` runs and runtime health is available
- **THEN** the CLI SHALL proceed with browser-open attempt and report success/fallback outcome

#### Scenario: Daemon not running
- **WHEN** `openstack studio` runs and runtime is not reachable
- **THEN** the CLI SHALL return an actionable message indicating how to start openstack before opening Studio

### Requirement: Cross-platform opener resolution
The Studio CLI command SHALL choose opener commands by operating system with testable deterministic mapping.

#### Scenario: Linux opener resolution
- **WHEN** running on Linux
- **THEN** the CLI SHALL attempt to open Studio using Linux opener semantics

#### Scenario: macOS opener resolution
- **WHEN** running on macOS
- **THEN** the CLI SHALL attempt to open Studio using macOS opener semantics

#### Scenario: Windows opener resolution
- **WHEN** running on Windows
- **THEN** the CLI SHALL attempt to open Studio using Windows opener semantics
