## ADDED Requirements

### Requirement: Studio SPA hosting and routing
The system SHALL serve a Studio single-page application from a reserved internal route namespace and SHALL return the Studio entry document for unknown Studio client-side routes under that namespace.

#### Scenario: Studio entry route resolves
- **WHEN** a user requests `GET /_localstack/studio`
- **THEN** the gateway SHALL return the Studio HTML entry document with a successful status code

#### Scenario: Studio client route fallback resolves
- **WHEN** a user requests `GET /_localstack/studio/services/s3`
- **THEN** the gateway SHALL return the same Studio HTML entry document so the client router can render the page

### Requirement: Studio theme and accessibility baseline
The Studio UI SHALL provide user-selectable light and dark themes, persist the selected theme locally, and meet keyboard navigation and semantic labeling requirements for core navigation and interaction controls.

#### Scenario: Theme selection persists across reload
- **WHEN** a user switches from light mode to dark mode in Studio
- **THEN** reloading Studio SHALL preserve dark mode and apply the same theme without requiring re-selection

#### Scenario: Keyboard navigation works for core controls
- **WHEN** a keyboard-only user navigates the main Studio navigation and action controls
- **THEN** focus order SHALL be logical and all actionable controls SHALL be operable via keyboard

### Requirement: Service catalog and capability visibility
Studio SHALL display a service catalog representing all services supported by openstack and SHALL indicate interaction support tier per service at minimum as `guided`, `raw`, or `coming-soon`.

#### Scenario: Catalog includes enabled services
- **WHEN** Studio loads against a running openstack instance
- **THEN** Studio SHALL render all enabled services from capability metadata in the catalog

#### Scenario: Service support tier is visible
- **WHEN** a service is listed in the Studio catalog
- **THEN** Studio SHALL display its support tier (`guided`, `raw`, or `coming-soon`) in the service card or detail view

### Requirement: Unified interaction console
Studio SHALL provide a raw interaction console for manually issuing HTTP requests to openstack endpoints with configurable method, path, headers, query parameters, and request body, and SHALL render response status, headers, and body.

#### Scenario: Raw request succeeds and response is visible
- **WHEN** a user submits a raw interaction request from Studio
- **THEN** Studio SHALL show the full response envelope including status code, response headers, and response body payload

#### Scenario: Raw request failure is diagnosable
- **WHEN** a raw interaction request fails due to transport or server error
- **THEN** Studio SHALL display a structured error state with sufficient detail for debugging

### Requirement: Guided service workflows
Studio SHALL provide guided interaction workflows for at least one operation path in each initial wave service class, and each workflow SHALL map to real openstack API calls rather than mocked behavior.

#### Scenario: Guided S3 flow executes real API path
- **WHEN** a user runs the guided S3 create bucket and upload object workflow
- **THEN** Studio SHALL issue real requests against openstack endpoints and show resulting object state

#### Scenario: Guided queue flow executes real API path
- **WHEN** a user runs the guided SQS create queue and send message workflow
- **THEN** Studio SHALL issue real requests and show resulting queue/message state transitions

### Requirement: Interaction history and replay
Studio SHALL capture interaction history entries for user-triggered requests and SHALL allow replaying a selected prior request with editable parameters.

#### Scenario: Interaction history captures requests
- **WHEN** a user executes multiple requests through Studio
- **THEN** Studio SHALL append corresponding history entries with timestamp, service, and status summary

#### Scenario: Replay seeds request editor
- **WHEN** a user selects a prior interaction for replay
- **THEN** Studio SHALL pre-populate the request editor with the original request values before execution
