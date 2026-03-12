## ADDED Requirements

### Requirement: Versioned guided-flow manifest schema
The system SHALL define a versioned Studio guided-flow manifest schema that is machine-validated and SHALL reject manifests that do not conform to the active schema version.

#### Scenario: Valid manifest is accepted
- **WHEN** a manifest file conforms to schema version `1.x`
- **THEN** the manifest SHALL be accepted by validation tooling and runtime loader

#### Scenario: Invalid manifest is rejected
- **WHEN** a manifest is missing required fields or violates field constraints
- **THEN** validation SHALL fail with actionable diagnostics and the manifest SHALL NOT be loaded

### Requirement: One manifest per supported service
The system SHALL maintain a guided-flow manifest for every supported service in the openstack service registry.

#### Scenario: Coverage check detects missing service manifest
- **WHEN** a service is present in the registry but has no corresponding manifest
- **THEN** coverage validation SHALL fail and report the missing service

### Requirement: Manifest defines flow lifecycle semantics
Each service manifest SHALL define one or more guided flows with explicit step execution, assertion verification, and cleanup semantics.

#### Scenario: L1 lifecycle flow includes verify and cleanup
- **WHEN** a service manifest defines baseline L1 guided flow
- **THEN** the flow SHALL include operation steps, at least one assertion, and cleanup steps

### Requirement: Safe expression binding model
Manifest interpolation SHALL support only approved expression sources (`inputs`, `context`, `captures`, approved built-ins) and SHALL disallow arbitrary dynamic code execution.

#### Scenario: Unsupported expression source is rejected
- **WHEN** a manifest contains unsupported expression syntax or source
- **THEN** validation SHALL fail with a clear unsupported-expression diagnostic

### Requirement: Schema evolution compatibility
The manifest contract SHALL follow explicit compatibility rules where minor versions are backward-compatible and major versions require migration guidance.

#### Scenario: Minor version manifest remains loadable
- **WHEN** runtime supports schema `1.2` and manifest uses `1.0`
- **THEN** the manifest SHALL remain loadable without migration

#### Scenario: Major version mismatch requires migration
- **WHEN** runtime supports schema `2.x` and manifest uses `1.x`
- **THEN** runtime SHALL report migration requirement and reject incompatible manifest loading
