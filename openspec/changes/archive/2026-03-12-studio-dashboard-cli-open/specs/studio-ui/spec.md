## MODIFIED Requirements

### Requirement: Studio SPA hosting and routing
The system SHALL serve a Studio single-page application from a reserved internal route namespace and SHALL return the Studio entry document for unknown Studio client-side routes under that namespace.

The Studio entry document SHALL boot a production dashboard frontend bundle (not a placeholder shell) and SHALL include required static assets for dashboard navigation, service interaction panels, and history/replay views.

#### Scenario: Studio entry route resolves
- **WHEN** a user requests `GET /_localstack/studio`
- **THEN** the gateway SHALL return the Studio HTML entry document with a successful status code

#### Scenario: Studio client route fallback resolves
- **WHEN** a user requests `GET /_localstack/studio/services/s3`
- **THEN** the gateway SHALL return the same Studio HTML entry document so the client router can render the page

#### Scenario: Dashboard frontend bundle is served
- **WHEN** a user opens Studio
- **THEN** the entry document SHALL load the dashboard frontend assets required for service catalog, operations, and history UI flows

### Requirement: Service catalog and capability visibility
Studio SHALL display a service catalog representing all services supported by openstack and SHALL indicate interaction support tier per service at minimum as `guided`, `raw`, or `coming-soon`.

Studio SHALL additionally surface guided-flow coverage and protocol metadata for each service in dashboard views to help users choose interaction mode.

#### Scenario: Catalog includes enabled services
- **WHEN** Studio loads against a running openstack instance
- **THEN** Studio SHALL render all enabled services from capability metadata in the catalog

#### Scenario: Service support tier is visible
- **WHEN** a service is listed in the Studio catalog
- **THEN** Studio SHALL display its support tier (`guided`, `raw`, or `coming-soon`) in the service card or detail view

#### Scenario: Guided coverage metadata is visible
- **WHEN** coverage metadata is available for services
- **THEN** Studio SHALL show coverage or maturity indicators in service cards or detail panels

### Requirement: Unified interaction console
Studio SHALL provide a raw interaction console for manually issuing HTTP requests to openstack endpoints with configurable method, path, headers, query parameters, and request body, and SHALL render response status, headers, and body.

The raw console SHALL be integrated into the dashboard service detail journey so users can switch between guided and raw operation paths without leaving Studio context.

#### Scenario: Raw request succeeds and response is visible
- **WHEN** a user submits a raw interaction request from Studio
- **THEN** Studio SHALL show the full response envelope including status code, response headers, and response body payload

#### Scenario: Raw request failure is diagnosable
- **WHEN** a raw interaction request fails due to transport or server error
- **THEN** Studio SHALL display a structured error state with sufficient detail for debugging

#### Scenario: Raw console preserves editor state on failure
- **WHEN** raw request execution fails
- **THEN** the request editor SHALL preserve submitted values for iterative retry and debugging

### Requirement: Interaction history and replay
Studio SHALL capture interaction history entries for user-triggered requests and SHALL allow replaying a selected prior request with editable parameters.

Studio SHALL support replay from both guided and raw pathways into the appropriate dashboard workspace with pre-populated context.

#### Scenario: Interaction history captures requests
- **WHEN** a user executes multiple requests through Studio
- **THEN** Studio SHALL append corresponding history entries with timestamp, service, and status summary

#### Scenario: Replay seeds request editor
- **WHEN** a user selects a prior interaction for replay
- **THEN** Studio SHALL pre-populate the request editor with the original request values before execution

#### Scenario: Replay restores guided context
- **WHEN** a user replays a guided flow interaction entry
- **THEN** Studio SHALL restore required guided input context and allow rerun from the guided workspace
