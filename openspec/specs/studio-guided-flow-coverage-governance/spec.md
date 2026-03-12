## Purpose

Define all-service guided-flow coverage governance and CI quality gates.

## ADDED Requirements

### Requirement: All-service guided coverage reporting
The system SHALL generate a Studio guided coverage report mapping every supported service to guided-flow coverage state and maturity level.

#### Scenario: Coverage report includes every registered service
- **WHEN** coverage report generation runs
- **THEN** every service from the runtime registry SHALL appear with guided coverage status

### Requirement: Minimum all-service guided completeness gate
CI SHALL enforce a minimum requirement that every supported service has at least one L1 guided flow manifest.

#### Scenario: Missing L1 flow blocks merge
- **WHEN** a supported service lacks L1 guided flow coverage
- **THEN** CI coverage gate SHALL fail and block merge

### Requirement: Manifest quality gate
CI SHALL enforce schema validity, semantic linting, and required flow semantics (steps, assertions, cleanup) for all manifests.

#### Scenario: Manifest missing required cleanup fails validation
- **WHEN** a manifest flow lacks required cleanup semantics for L1 classification
- **THEN** validation gate SHALL fail with explicit quality diagnostic

### Requirement: Protocol representative runtime validation
CI SHALL execute guided runtime validation scenarios covering representative services for each protocol class.

#### Scenario: Protocol class regression blocks merge
- **WHEN** representative guided flow test for any protocol class fails
- **THEN** CI SHALL fail and block merge

### Requirement: Governance drift detection
The governance system SHALL detect mismatches between service registry changes and manifest coverage updates.

#### Scenario: New service added without manifest
- **WHEN** registry introduces a new supported service without manifest update
- **THEN** governance checks SHALL fail and identify required manifest additions
