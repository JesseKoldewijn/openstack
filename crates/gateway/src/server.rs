use std::cell::RefCell;
use std::collections::HashMap;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::IntoResponse;
use axum::response::Response;
use bytes::Bytes;
use http_body_util::BodyStream;
use openstack_aws_protocol::{
    AwsProtocol, ec2::parse_ec2_request, json::parse_json_request, query::parse_query_request,
    rest_json::parse_rest_json_request, rest_xml::parse_rest_xml_request,
};
use openstack_config::Config;
use openstack_service_framework::traits::ResponseBody;
use openstack_service_framework::{ServicePluginManager, SpooledBody};
use openstack_state::StateManager;
use rand::RngExt;
use rand::SeedableRng;
use rand::rngs::SmallRng;
use tower::ServiceBuilder;
use tower_http::compression::CompressionLayer;
use tracing::{debug, error, info, warn};

use crate::context::RequestContext;
use crate::cors::CorsHandler;
use crate::sigv4::{
    DEFAULT_ACCESS_KEY, DEFAULT_REGION, access_key_to_account_id, is_valid_region, parse_sigv4_auth,
};

const STUDIO_SPA: &str = r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width,initial-scale=1" />
  <title>openstack studio</title>
  <link rel="stylesheet" href="/_localstack/studio/assets/app.css" />
</head>
<body>
  <div id="studio-app">Loading Studio dashboard...</div>
  <script src="/_localstack/studio/assets/app.js"></script>
</body>
</html>
"#;

const STUDIO_ASSET_JS: &str = r#"(function () {
  const root = document.getElementById('studio-app');
  if (!root) return;

  const state = {
    services: [],
    flowCatalog: [],
    flowCoverage: [],
    selectedService: null,
    flowDefinition: null,
    history: [],
    raw: { method: 'GET', path: '/_localstack/health', body: '' },
    response: null,
  };

  function esc(v) {
    return String(v ?? '').replace(/[&<>\"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
  }

  async function getJson(path) {
    const r = await fetch(path, { headers: { 'accept': 'application/json' } });
    if (!r.ok) throw new Error(path + ' failed: ' + r.status);
    return r.json();
  }

  function mergeServiceData() {
    const byFlow = new Map(state.flowCatalog.map((x) => [x.service, x]));
    const byCoverage = new Map(state.flowCoverage.map((x) => [x.service, x]));
    return state.services.map((s) => {
      const flow = byFlow.get(s.name) || { protocol: 'unknown', flow_count: 0, maturity: 'none' };
      const coverage = byCoverage.get(s.name) || { quality: 'unknown', l1_flows: 0 };
      return {
        name: s.name,
        status: s.status,
        tier: s.support_tier,
        protocol: flow.protocol,
        flows: flow.flow_count,
        quality: coverage.quality,
      };
    });
  }

  function render() {
    const cards = mergeServiceData();
    const selected = state.selectedService;
    const selectedCard = cards.find((c) => c.name === selected);

    root.innerHTML = `
      <div class=\"studio-layout\">
        <header class=\"studio-header\">
          <h1>OpenStack Studio</h1>
          <p>Service dashboard for guided and raw operations</p>
        </header>
        <main class=\"studio-grid\">
          <section class=\"studio-panel\">
            <h2>Services</h2>
            <div class=\"service-list\">
              ${cards.map((c) => `
                <button class=\"service-card ${selected === c.name ? 'active' : ''}\" data-service=\"${esc(c.name)}\">
                  <strong>${esc(c.name)}</strong>
                  <span>Status: ${esc(c.status)}</span>
                  <span>Tier: ${esc(c.tier)}</span>
                  <span>Protocol: ${esc(c.protocol)}</span>
                  <span>Flows: ${esc(c.flows)}</span>
                  <span>Coverage: ${esc(c.quality)}</span>
                </button>
              `).join('')}
            </div>
          </section>
          <section class=\"studio-panel\">
            <h2>Service Detail ${selectedCard ? '(' + esc(selectedCard.name) + ')' : ''}</h2>
            ${selected ? renderDetail() : '<p>Select a service to run guided or raw operations.</p>'}
          </section>
          <section class=\"studio-panel\">
            <h2>History</h2>
            <div class=\"history-list\">
              ${state.history.length === 0 ? '<p>No interactions yet.</p>' : state.history.map((h, i) =>
                `<button class=\"history-item\" data-history=\"${i}\">${esc(h.service)} - ${esc(h.method)} ${esc(h.path)} (${esc(h.status)})</button>`
              ).join('')}
            </div>
          </section>
        </main>
      </div>
    `;

    root.querySelectorAll('[data-service]').forEach((btn) => {
      btn.addEventListener('click', async () => {
        state.selectedService = btn.getAttribute('data-service');
        try {
          state.flowDefinition = await getJson('/_localstack/studio-api/flows/' + state.selectedService);
        } catch (e) {
          state.flowDefinition = { service: state.selectedService, flows: [], inputs: [] };
        }
        render();
      });
    });

    root.querySelectorAll('[data-history]').forEach((btn) => {
      btn.addEventListener('click', () => {
        const idx = Number(btn.getAttribute('data-history'));
        const item = state.history[idx];
        if (!item) return;
        state.raw.method = item.method;
        state.raw.path = item.path;
        state.raw.body = item.body || '';
        render();
      });
    });

    const rawRun = root.querySelector('[data-run-raw]');
    if (rawRun) rawRun.addEventListener('click', runRaw);
    const flowRun = root.querySelector('[data-run-flow]');
    if (flowRun) flowRun.addEventListener('click', runGuided);
  }

  function renderDetail() {
    const flow = state.flowDefinition;
    return `
      <div class=\"detail-grid\">
        <div>
          <h3>Guided Flow</h3>
          <p>Flows available: ${esc(flow && flow.flows ? flow.flows.length : 0)}</p>
          <button data-run-flow ${!flow || !flow.flows || flow.flows.length === 0 ? 'disabled' : ''}>Run first guided flow</button>
        </div>
        <div>
          <h3>Raw Request</h3>
          <label>Method <input id=\"raw-method\" value=\"${esc(state.raw.method)}\"/></label>
          <label>Path <input id=\"raw-path\" value=\"${esc(state.raw.path)}\"/></label>
          <label>Body <textarea id=\"raw-body\">${esc(state.raw.body)}</textarea></label>
          <button data-run-raw>Run raw request</button>
          ${state.response ? `<pre>${esc(JSON.stringify(state.response, null, 2))}</pre>` : ''}
        </div>
      </div>
    `;
  }

  async function runRaw() {
    const methodInput = document.getElementById('raw-method');
    const pathInput = document.getElementById('raw-path');
    const bodyInput = document.getElementById('raw-body');
    state.raw.method = (methodInput && methodInput.value || 'GET').toUpperCase();
    state.raw.path = (pathInput && pathInput.value || '/_localstack/health');
    state.raw.body = bodyInput ? bodyInput.value : '';

    const opts = { method: state.raw.method, headers: {} };
    if (state.raw.body) opts.body = state.raw.body;
    const r = await fetch(state.raw.path, opts);
    const text = await r.text();
    state.response = { status: r.status, body: text };
    state.history.unshift({
      service: state.selectedService || 'raw',
      method: state.raw.method,
      path: state.raw.path,
      body: state.raw.body,
      status: r.status,
    });
    state.history = state.history.slice(0, 20);
    render();
  }

  async function runGuided() {
    if (!state.flowDefinition || !state.flowDefinition.flows || state.flowDefinition.flows.length === 0) return;
    const firstFlow = state.flowDefinition.flows[0];
    for (const step of (firstFlow.steps || [])) {
      const op = step.operation || {};
      const method = (op.method || 'GET').toUpperCase();
      const path = (op.path || '/').replace('{{inputs.resource_name}}', 'studio-resource');
      const body = op.body || undefined;
      const r = await fetch(path, { method, body });
      state.history.unshift({
        service: state.selectedService || 'guided',
        method,
        path,
        body: body || '',
        status: r.status,
      });
    }
    state.history = state.history.slice(0, 20);
    render();
  }

  async function bootstrap() {
    const [services, flowCatalog, flowCoverage] = await Promise.all([
      getJson('/_localstack/studio-api/services'),
      getJson('/_localstack/studio-api/flows/catalog'),
      getJson('/_localstack/studio-api/flows/coverage'),
    ]);
    state.services = services.services || [];
    state.flowCatalog = flowCatalog.services || [];
    state.flowCoverage = flowCoverage.services || [];
    render();
  }

  bootstrap().catch((err) => {
    root.innerHTML = '<pre>Studio dashboard failed to load: ' + esc(err.message || String(err)) + '</pre>';
  });
})();
"#;

const STUDIO_ASSET_CSS: &str = r#":root{color-scheme:light dark;--bg:#0f172a;--fg:#e2e8f0;--card:#1e293b;--muted:#94a3b8;--accent:#22c55e}*{box-sizing:border-box}body{margin:0;font-family:ui-sans-serif,system-ui,sans-serif;background:linear-gradient(120deg,#0f172a,#111827);color:var(--fg)}.studio-layout{max-width:1200px;margin:0 auto;padding:20px}.studio-header h1{margin:0}.studio-header p{color:var(--muted)}.studio-grid{display:grid;grid-template-columns:1fr 1.4fr 1fr;gap:16px}.studio-panel{background:color-mix(in oklab,var(--card) 92%,black);border:1px solid #334155;border-radius:12px;padding:12px;min-height:260px}.service-list{display:grid;gap:8px}.service-card{display:grid;text-align:left;gap:2px;padding:8px;border:1px solid #334155;background:#0b1220;color:var(--fg);border-radius:8px;cursor:pointer}.service-card.active{border-color:var(--accent)}.detail-grid{display:grid;gap:12px}label{display:grid;gap:6px;margin:6px 0}input,textarea{width:100%;background:#0b1220;color:var(--fg);border:1px solid #334155;border-radius:8px;padding:8px}button{background:#0b1220;color:var(--fg);border:1px solid #334155;border-radius:8px;padding:8px;cursor:pointer}button:disabled{opacity:.5;cursor:not-allowed}.history-list{display:grid;gap:8px}.history-item{text-align:left}pre{white-space:pre-wrap;overflow:auto;background:#0b1220;padding:8px;border-radius:8px}@media(max-width:1024px){.studio-grid{grid-template-columns:1fr}}"#;
const STUDIO_GUIDED_MAX_PAYLOAD_BYTES: usize = 256 * 1024;

// Thread-local fast RNG — seeded once per thread from the OS RNG, avoiding a
// getrandom syscall on every request.
thread_local! {
    static FAST_RNG: RefCell<SmallRng> = RefCell::new(SmallRng::from_rng(&mut rand::rng()));
}

/// Generate a UUID v4-formatted request ID using the thread-local fast RNG.
///
/// ~10x faster than `Uuid::new_v4()` which hits the kernel CSPRNG on every call.
fn fast_request_id() -> String {
    FAST_RNG.with(|rng| {
        let mut rng = rng.borrow_mut();
        let mut b = [0u8; 16];
        rng.fill(&mut b);
        // Set UUID v4 version and variant bits for RFC 4122 compliance.
        b[6] = (b[6] & 0x0f) | 0x40;
        b[8] = (b[8] & 0x3f) | 0x80;
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut out = [0u8; 36];
        let mut o = 0;
        for (i, &byte) in b.iter().enumerate() {
            if i == 4 || i == 6 || i == 8 || i == 10 {
                out[o] = b'-';
                o += 1;
            }
            out[o] = HEX[(byte >> 4) as usize];
            o += 1;
            out[o] = HEX[(byte & 0x0f) as usize];
            o += 1;
        }
        // SAFETY: HEX table and '-' are all valid ASCII/UTF-8.
        unsafe { String::from_utf8_unchecked(out.to_vec()) }
    })
}

/// Adapter that converts `http_body_util::BodyStream<axum::body::Body>` into
/// a `futures_core::Stream<Item = Result<Bytes, io::Error>>` suitable for
/// `SpooledBody::write_from_stream()`.
struct BodyStreamAdapter {
    inner: BodyStream<Body>,
    bytes_read: usize,
    max_bytes: Option<usize>,
}

impl BodyStreamAdapter {
    fn new(body: Body, max_bytes: Option<usize>) -> Self {
        Self {
            inner: BodyStream::new(body),
            bytes_read: 0,
            max_bytes,
        }
    }
}

impl futures_core::Stream for BodyStreamAdapter {
    type Item = Result<Bytes, io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            match Pin::new(&mut self.inner).poll_next(cx) {
                Poll::Ready(Some(Ok(frame))) => {
                    if let Ok(data) = frame.into_data() {
                        self.bytes_read = self.bytes_read.saturating_add(data.len());
                        if self.max_bytes.is_some_and(|limit| self.bytes_read > limit) {
                            return Poll::Ready(Some(Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                "guided execution payload exceeds configured limit",
                            ))));
                        }
                        return Poll::Ready(Some(Ok(data)));
                    }
                    // Skip non-data frames (trailers, etc.)
                    continue;
                }
                Poll::Ready(Some(Err(e))) => return Poll::Ready(Some(Err(io::Error::other(e)))),
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

/// The main HTTP gateway for openstack.
pub struct Gateway {
    config: Config,
    plugin_manager: ServicePluginManager,
    shutdown_tx: tokio::sync::broadcast::Sender<()>,
}

/// Shared application state passed to all axum handlers.
#[derive(Clone)]
struct AppState {
    config: Config,
    plugin_manager: ServicePluginManager,
    cors: Arc<CorsHandler>,
    internal_api_router: Router,
}

impl Gateway {
    pub fn new(config: Config, plugin_manager: ServicePluginManager) -> Self {
        let (shutdown_tx, _) = tokio::sync::broadcast::channel(16);
        Self {
            config,
            plugin_manager,
            shutdown_tx,
        }
    }

    /// Build the axum Router for this gateway (useful for testing).
    fn build_app(&self) -> Router {
        let cors = Arc::new(CorsHandler::new(&self.config));
        let internal_state = openstack_internal_api::ApiState::new(
            self.config.clone(),
            self.plugin_manager.clone(),
            self.shutdown_tx.clone(),
        );
        let internal_api_router = openstack_internal_api::internal_api_router(internal_state);
        let app_state = AppState {
            config: self.config.clone(),
            plugin_manager: self.plugin_manager.clone(),
            cors,
            internal_api_router,
        };
        Router::new()
            .fallback(handle_request)
            .layer(ServiceBuilder::new().layer(CompressionLayer::new()))
            .with_state(app_state)
    }

    #[doc(hidden)]
    pub fn build_app_for_tests(&self) -> Router {
        self.build_app()
    }

    /// Run the gateway using a pre-bound listener and an external shutdown signal.
    /// Useful for integration tests where you need to control port allocation.
    pub async fn run_with_listener(
        self,
        listener: tokio::net::TcpListener,
        mut shutdown: tokio::sync::oneshot::Receiver<()>,
    ) -> Result<(), anyhow::Error> {
        if self.config.eager_service_loading {
            info!("Eagerly starting all services...");
            self.plugin_manager.start_all().await;
        }
        let app = self.build_app();
        let mut api_shutdown = self.shutdown_tx.subscribe();
        tokio::select! {
            result = axum::serve(listener, app) => { result?; }
            _ = &mut shutdown => {
                info!("Shutdown signal received");
                self.plugin_manager.stop_all().await;
            }
            _ = api_shutdown.recv() => {
                info!("Shutdown requested via internal API");
                self.plugin_manager.stop_all().await;
            }
        }
        Ok(())
    }

    pub async fn run(self, state_manager: StateManager) -> Result<(), anyhow::Error> {
        let config = self.config.clone();

        // Eager service loading if configured
        if config.eager_service_loading {
            info!("Eagerly starting all services...");
            self.plugin_manager.start_all().await;
        }

        let app = self.build_app();
        let mut api_shutdown = self.shutdown_tx.subscribe();

        // Bind to all configured addresses
        let addrs = config.gateway_listen.clone();
        let mut handles = Vec::new();

        for addr in addrs {
            let app_clone = app.clone();
            let addr_str = addr.to_string();
            let listener = tokio::net::TcpListener::bind(addr).await?;
            info!("Gateway listening on {}", addr_str);

            let handle = tokio::spawn(async move { axum::serve(listener, app_clone).await });
            handles.push(handle);
        }

        // Wait for shutdown signal
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                info!("Shutdown signal received");
            }
            _ = api_shutdown.recv() => {
                info!("Shutdown requested via internal API");
            }
        }

        // Save state on shutdown
        state_manager.save_on_shutdown().await?;

        // Stop all services
        self.plugin_manager.stop_all().await;

        Ok(())
    }
}

/// The main request handler - processes all incoming AWS API requests.
async fn handle_request(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    req: axum::extract::Request,
) -> Response {
    let request_start = std::time::Instant::now();

    // Fast-path early exits: check cheap conditions BEFORE allocating a
    // request ID, owned path string, or parsing query parameters.

    // Handle CORS preflight — only needs method + headers, zero allocs.
    if CorsHandler::is_preflight(&method, &headers) {
        let mut resp_headers = HeaderMap::new();
        state.cors.add_cors_headers(
            &mut resp_headers,
            headers.get("origin").and_then(|v| v.to_str().ok()),
        );
        let mut response = StatusCode::OK.into_response();
        *response.headers_mut() = resp_headers;
        return response;
    }

    // Studio SPA/asset routes and internal API routes are resolved before
    // any allocation.  `uri.path()` returns a `&str` into the URI — no
    // heap allocation until we actually need an owned `String`.
    let uri = req.uri().clone();
    let path_str = uri.path();

    if is_studio_asset_route(path_str) {
        return studio_asset_response(path_str);
    }

    if is_studio_spa_route(path_str) {
        return studio_spa_response();
    }

    // Internal API routes (/_localstack/*) — allocate path + query only here.
    if path_str.starts_with("/_localstack/") {
        // Check studio guided execution payload size before reading body.
        if is_studio_guided_execution_route(path_str)
            && headers
                .get(axum::http::header::CONTENT_LENGTH)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<usize>().ok())
                .is_some_and(|len| len > STUDIO_GUIDED_MAX_PAYLOAD_BYTES)
        {
            return (
                StatusCode::PAYLOAD_TOO_LARGE,
                "guided execution payload exceeds configured limit",
            )
                .into_response();
        }

        let guided_limit = if is_studio_guided_execution_route(path_str) {
            Some(STUDIO_GUIDED_MAX_PAYLOAD_BYTES)
        } else {
            None
        };

        // Stream and buffer the body for internal API dispatch.
        let threshold = state.config.body_spool_threshold_bytes;
        let mut spooled = SpooledBody::new(threshold);
        let stream = BodyStreamAdapter::new(req.into_body(), guided_limit);
        if let Err(e) = spooled.write_from_stream(stream).await {
            if e.kind() == io::ErrorKind::InvalidData {
                return (
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "guided execution payload exceeds configured limit",
                )
                    .into_response();
            }
            error!("Failed to read request body: {}", e);
            return (StatusCode::BAD_REQUEST, "Failed to read request body").into_response();
        }
        let body_bytes = match spooled.into_bytes() {
            Ok(b) => b,
            Err(e) => {
                error!("Failed to materialize request body: {}", e);
                return (StatusCode::BAD_REQUEST, "Failed to read request body").into_response();
            }
        };

        let path = path_str.to_string();
        let query_string = uri.query().unwrap_or("");
        let query_params: HashMap<String, String> = if query_string.is_empty() {
            HashMap::new()
        } else {
            serde_urlencoded::from_str(query_string).unwrap_or_default()
        };

        return handle_internal_api(path, &method, &headers, &query_params, &body_bytes, &state)
            .await;
    }

    // --- AWS service request path ---
    // Only now do we allocate request ID, owned path, and parse query params.
    let request_id = fast_request_id();
    let path = path_str.to_string();
    let query_string = uri.query().unwrap_or("");

    // Parse query parameters
    let query_params: HashMap<String, String> = if query_string.is_empty() {
        HashMap::new()
    } else {
        serde_urlencoded::from_str(query_string).unwrap_or_default()
    };

    // Check studio guided execution payload size (for any studio route that
    // slipped through the /_localstack/ prefix check above — shouldn't happen
    // in practice, but kept as a safety net).
    if is_studio_guided_execution_route(&path)
        && headers
            .get(axum::http::header::CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<usize>().ok())
            .is_some_and(|len| len > STUDIO_GUIDED_MAX_PAYLOAD_BYTES)
    {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            "guided execution payload exceeds configured limit",
        )
            .into_response();
    }

    let guided_limit = if is_studio_guided_execution_route(&path) {
        Some(STUDIO_GUIDED_MAX_PAYLOAD_BYTES)
    } else {
        None
    };

    // Stream the request body into a SpooledBody
    let threshold = state.config.body_spool_threshold_bytes;
    let mut spooled = SpooledBody::new(threshold);
    let stream = BodyStreamAdapter::new(req.into_body(), guided_limit);
    if let Err(e) = spooled.write_from_stream(stream).await {
        if e.kind() == io::ErrorKind::InvalidData {
            return (
                StatusCode::PAYLOAD_TOO_LARGE,
                "guided execution payload exceeds configured limit",
            )
                .into_response();
        }
        error!("Failed to read request body: {}", e);
        return (StatusCode::BAD_REQUEST, "Failed to read request body").into_response();
    }

    // Materialize raw_body as Bytes for protocol parsing.
    // For S3 object-body requests (PUT/POST to a bucket+key path) the body
    // is binary object data — never XML or JSON — so we skip the copy and
    // let the S3 provider stream it directly from the SpooledBody instead.
    //
    // For all other services we consume the SpooledBody via `into_bytes()`,
    // replacing it with an empty sentinel.  No non-S3 provider ever reads
    // `spooled_body`, so this avoids keeping two copies of the body in
    // memory (the materialised `Bytes` and the original spool buffer).
    let is_s3_body = is_s3_object_body_request(&method, &path, &headers, &query_params);
    let body_bytes = if is_s3_body {
        // Keep the body in the SpooledBody; pass an empty slice to parsers.
        Bytes::new()
    } else {
        // Swap the data-bearing spool for an empty sentinel and consume it,
        // so only one copy of the body lives in memory at a time.
        let owned = std::mem::replace(&mut spooled, SpooledBody::new(0));
        match owned.into_bytes() {
            Ok(b) => b,
            Err(e) => {
                error!("Failed to materialize request body: {}", e);
                return (StatusCode::BAD_REQUEST, "Failed to read request body").into_response();
            }
        }
    };

    // Extract the origin header before consuming headers (needed for CORS later).
    let origin_header: Option<String> = headers
        .get("origin")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);

    // Build request context
    let context_start = std::time::Instant::now();
    let ctx = match build_request_context(
        &method,
        path,
        query_params,
        headers,
        &body_bytes,
        &request_id,
        &state.config,
        // Only S3 object-body requests need spooled_body; for all others the
        // body was already consumed into body_bytes above.
        if is_s3_body { Some(spooled) } else { None },
    ) {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };
    let context_latency_us = context_start.elapsed().as_micros();
    let protocol = ctx.protocol.clone();

    debug!(
        request_id = %request_id,
        service = %ctx.service,
        operation = %ctx.operation,
        region = %ctx.region,
        account_id = %ctx.account_id,
        context_latency_us = context_latency_us,
        "Dispatching request"
    );

    // Convert to service framework context (consumes ctx — SpooledBody is not Clone)
    let svc_ctx = ctx.to_service_request_context();

    // Dispatch to the service provider
    let start = std::time::Instant::now();
    let result = state.plugin_manager.dispatch(&svc_ctx).await;
    let latency_ms = start.elapsed().as_millis();
    let total_latency_ms = request_start.elapsed().as_millis();

    let (status, resp_body, content_type, extra_headers) = match result {
        Ok(response) => {
            info!(
                request_id = %request_id,
                service = %svc_ctx.service,
                operation = %svc_ctx.operation,
                status = response.status_code,
                latency_ms = latency_ms,
                total_latency_ms = total_latency_ms,
                "Request completed"
            );
            (
                StatusCode::from_u16(response.status_code).unwrap_or(StatusCode::OK),
                response.body,
                response.content_type,
                response.headers,
            )
        }
        Err(e) => {
            use openstack_service_framework::traits::DispatchError;
            let (code, message, http_status) = match &e {
                DispatchError::NotImplemented(op) => (
                    "NotImplemented",
                    format!("Operation '{}' is not implemented", op),
                    501u16,
                ),
                DispatchError::ServiceNotFound(svc) => (
                    "InternalFailure",
                    format!(
                        "Service '{}' is not enabled. Please check your 'SERVICES' configuration variable.",
                        svc
                    ),
                    501,
                ),
                DispatchError::ServiceUnavailable(msg) => ("ServiceUnavailable", msg.clone(), 503),
                DispatchError::ProviderError(msg) => ("InternalFailure", msg.clone(), 500),
                DispatchError::SerializationError(msg) => ("InternalFailure", msg.clone(), 500),
            };

            warn!(
                request_id = %request_id,
                service = %svc_ctx.service,
                operation = %svc_ctx.operation,
                error = %e,
                http_status = http_status,
                latency_ms = latency_ms,
                total_latency_ms = total_latency_ms,
                "Request failed"
            );

            let (status_code, body, ct) = openstack_aws_protocol::serialize_error(
                &protocol,
                code,
                &message,
                http_status,
                &request_id,
            );
            (
                StatusCode::from_u16(status_code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                ResponseBody::Buffered(body),
                std::borrow::Cow::Borrowed(ct),
                Vec::new(),
            )
        }
    };

    // Build the response based on body variant
    let mut response = match resp_body {
        ResponseBody::Buffered(bytes) => Response::builder()
            .status(status)
            .header("content-type", &*content_type)
            .header("x-amzn-requestid", &request_id)
            .body(Body::from(bytes))
            .unwrap_or_default(),
        ResponseBody::Streaming {
            stream,
            content_length,
        } => {
            let mut builder = Response::builder()
                .status(status)
                .header("content-type", &*content_type)
                .header("x-amzn-requestid", &request_id)
                // Prevent tower-http CompressionLayer from compressing binary
                // object data (S3 GET). Compressing random/incompressible bytes
                // burns CPU for zero benefit and adds significant latency.
                .header("content-encoding", "identity");
            if let Some(len) = content_length {
                builder = builder.header("content-length", len.to_string());
            }
            builder.body(Body::from_stream(stream)).unwrap_or_default()
        }
    };

    // Add extra headers from the provider
    for (key, value) in extra_headers {
        if let Ok(v) = axum::http::HeaderValue::from_str(&value) {
            response.headers_mut().insert(
                axum::http::HeaderName::from_bytes(key.as_bytes()).unwrap(),
                v,
            );
        }
    }

    // Add CORS headers
    state
        .cors
        .add_cors_headers(response.headers_mut(), origin_header.as_deref());

    response
}

fn is_studio_spa_route(path: &str) -> bool {
    (path == "/_localstack/studio"
        || path == "/_localstack/studio/"
        || path.starts_with("/_localstack/studio/"))
        && !path.starts_with("/_localstack/studio/assets/")
}

fn is_studio_asset_route(path: &str) -> bool {
    path.starts_with("/_localstack/studio/assets/")
}

fn studio_spa_response() -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/html; charset=utf-8")
        .header("cache-control", "no-cache")
        .header("etag", "\"studio-shell-v1\"")
        .body(Body::from(STUDIO_SPA))
        .unwrap_or_default()
}

fn studio_asset_response(path: &str) -> Response {
    let (status, content_type, body, cache_control) = match path {
        "/_localstack/studio/assets/app.js" => (
            StatusCode::OK,
            "application/javascript; charset=utf-8",
            STUDIO_ASSET_JS,
            "public, max-age=31536000, immutable",
        ),
        "/_localstack/studio/assets/app.css" => (
            StatusCode::OK,
            "text/css; charset=utf-8",
            STUDIO_ASSET_CSS,
            "public, max-age=31536000, immutable",
        ),
        _ => (
            StatusCode::NOT_FOUND,
            "text/plain; charset=utf-8",
            "Not found",
            "no-cache",
        ),
    };

    Response::builder()
        .status(status)
        .header("content-type", content_type)
        .header("cache-control", cache_control)
        .header("etag", "\"studio-asset-v1\"")
        .body(Body::from(body))
        .unwrap_or_default()
}

#[allow(clippy::result_large_err)]
fn build_request_context(
    method: &Method,
    path: String,
    query_params: HashMap<String, String>,
    headers: HeaderMap,
    body: &Bytes,
    request_id: &str,
    config: &Config,
    spooled_body: Option<SpooledBody>,
) -> Result<RequestContext, Response> {
    // Parse SigV4 Authorization or inject default.
    // SigV4Auth borrows from the auth header string — no allocations here.
    // We convert to owned Strings only at RequestContext construction.
    let (access_key, region, service_from_auth): (&str, &str, Option<&str>) =
        if let Some(auth) = headers.get("authorization").and_then(|v| v.to_str().ok()) {
            if let Some(sigv4) = parse_sigv4_auth(auth) {
                (sigv4.access_key, sigv4.region, Some(sigv4.service))
            } else {
                (DEFAULT_ACCESS_KEY, DEFAULT_REGION, None)
            }
        } else {
            (DEFAULT_ACCESS_KEY, DEFAULT_REGION, None)
        };

    // Derive account ID from access key
    let account_id = access_key_to_account_id(access_key);

    // Determine the target service
    let service = detect_service(&path, &query_params, &headers, body, service_from_auth);

    // Validate / normalize region
    let region = if config.allow_nonstandard_regions || is_valid_region(region) {
        region.to_string()
    } else {
        warn!("Invalid region '{}', falling back to us-east-1", region);
        DEFAULT_REGION.to_string()
    };

    // Determine the protocol used by this service
    let protocol = AwsProtocol::from_service(&service);

    // Parse the request body according to protocol
    let (operation, params) =
        match parse_operation_and_params(method, &path, &query_params, &headers, body, &protocol) {
            Ok(result) => result,
            Err(e) => {
                error!("Failed to parse request: {}", e);
                let body = format!("Failed to parse request: {}", e);
                return Err((StatusCode::BAD_REQUEST, body).into_response());
            }
        };

    Ok(RequestContext {
        service,
        operation,
        region,
        account_id,
        access_key: access_key.to_string(),
        protocol,
        params,
        raw_body: if body.is_empty() {
            None
        } else {
            Some(body.clone())
        },
        headers,
        path,
        method: method.to_string(),
        query_params,
        request_id: request_id.to_string(),
        spooled_body,
    })
}

/// Detect which AWS service is being targeted.
fn detect_service(
    path: &str,
    query_params: &HashMap<String, String>,
    headers: &HeaderMap,
    body: &Bytes,
    service_from_auth: Option<&str>,
) -> String {
    // 1. Authorization header credential scope (highest priority)
    if let Some(svc) = service_from_auth {
        return normalize_service_name(svc);
    }

    // 2. Host header: sqs.us-east-1.localhost.localstack.cloud
    if let Some(host) = headers.get("host").and_then(|v| v.to_str().ok()) {
        let host = host.split(':').next().unwrap_or(host);
        let parts: Vec<&str> = host.split('.').collect();
        if parts.len() >= 2 {
            let potential_service = parts[0];
            if potential_service
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
                && is_known_service(potential_service)
            {
                // Known service names have a static equivalent — use it.
                if let Some(s) = known_service_static(potential_service) {
                    return s.to_string();
                }
                return potential_service.to_string();
            }
        }
    }

    // 3. X-Amz-Target header: "DynamoDB_20120810.GetItem"
    if path == "/"
        && query_params.is_empty()
        && body.is_empty()
        && !headers.contains_key("x-amz-target")
    {
        return "s3".to_string();
    }

    if let Some(target) = headers.get("x-amz-target").and_then(|v| v.to_str().ok())
        && let Some(svc) = service_from_target(target)
    {
        return svc.to_string();
    }

    // 4. Query protocol Action (POST form body or query string)
    if let Some(svc) = service_from_query_action(query_params, body) {
        return svc.to_string();
    }

    // 5. URL path patterns
    if let Some(svc) = service_from_path(path) {
        return svc.to_string();
    }

    // 6. S3 path-style heuristic for unsigned endpoint-url calls
    let trimmed = path.trim_start_matches('/');
    if !trimmed.is_empty() {
        return "s3".to_string();
    }

    "unknown".to_string()
}

fn normalize_service_name(service: &str) -> String {
    if service.eq_ignore_ascii_case("es") {
        return "opensearch".to_string();
    }
    // AWS SDKs always send lowercase service names in credentials, so the
    // common path avoids `to_ascii_lowercase`'s character-by-character scan.
    if service.bytes().all(|b| !b.is_ascii_uppercase()) {
        // Try to resolve to a known static string to avoid allocating.
        if let Some(s) = known_service_static(service) {
            s.to_string()
        } else {
            service.to_string()
        }
    } else {
        service.to_ascii_lowercase()
    }
}

/// Return the canonical `&'static str` for a known service name.
/// This allows callers to avoid allocating when the input matches a known service.
fn known_service_static(name: &str) -> Option<&'static str> {
    match name {
        "s3" => Some("s3"),
        "sqs" => Some("sqs"),
        "sns" => Some("sns"),
        "dynamodb" => Some("dynamodb"),
        "lambda" => Some("lambda"),
        "iam" => Some("iam"),
        "sts" => Some("sts"),
        "kms" => Some("kms"),
        "cloudformation" => Some("cloudformation"),
        "cloudwatch" => Some("cloudwatch"),
        "logs" => Some("logs"),
        "kinesis" => Some("kinesis"),
        "firehose" => Some("firehose"),
        "events" => Some("events"),
        "states" => Some("states"),
        "apigateway" => Some("apigateway"),
        "ec2" => Some("ec2"),
        "route53" => Some("route53"),
        "ses" => Some("ses"),
        "ssm" => Some("ssm"),
        "secretsmanager" => Some("secretsmanager"),
        "acm" => Some("acm"),
        "ecr" => Some("ecr"),
        "opensearch" => Some("opensearch"),
        "redshift" => Some("redshift"),
        "elasticache" => Some("elasticache"),
        "rds" => Some("rds"),
        _ => None,
    }
}

/// Zero-copy scan of URL-encoded body bytes for `Action=<value>`.
/// Returns the value slice without allocating.
fn extract_action_value(body: &[u8]) -> Option<&str> {
    let s = std::str::from_utf8(body).ok()?;
    s.split('&')
        .find_map(|segment| segment.strip_prefix("Action="))
}

fn service_from_query_action(
    query_params: &HashMap<String, String>,
    body: &Bytes,
) -> Option<&'static str> {
    let action: &str = if let Some(a) = query_params.get("Action").map(String::as_str) {
        a
    } else {
        extract_action_value(body)?
    };

    match action {
        // SQS
        "CreateQueue"
        | "DeleteQueue"
        | "GetQueueUrl"
        | "GetQueueAttributes"
        | "SetQueueAttributes"
        | "SendMessage"
        | "ReceiveMessage"
        | "DeleteMessage"
        | "PurgeQueue"
        | "ListQueues"
        | "SendMessageBatch"
        | "DeleteMessageBatch"
        | "ChangeMessageVisibility"
        | "ChangeMessageVisibilityBatch" => Some("sqs"),
        // STS
        "GetCallerIdentity" | "AssumeRole" => Some("sts"),
        // SNS
        "CreateTopic" | "DeleteTopic" | "Publish" | "Subscribe" | "Unsubscribe" | "ListTopics"
        | "SetTopicAttributes" | "GetTopicAttributes" => Some("sns"),
        // IAM
        "CreateRole" | "DeleteRole" | "ListRoles" | "GetRole" | "CreateUser" | "DeleteUser"
        | "ListUsers" | "GetUser" => Some("iam"),
        _ => None,
    }
}

fn is_known_service(name: &str) -> bool {
    matches!(
        name,
        "s3" | "sqs"
            | "sns"
            | "dynamodb"
            | "lambda"
            | "iam"
            | "sts"
            | "kms"
            | "cloudformation"
            | "cloudwatch"
            | "logs"
            | "kinesis"
            | "firehose"
            | "events"
            | "states"
            | "apigateway"
            | "ec2"
            | "route53"
            | "ses"
            | "ssm"
            | "secretsmanager"
            | "acm"
            | "ecr"
            | "opensearch"
            | "redshift"
            | "elasticache"
            | "rds"
    )
}

/// Derive the AWS service name from an `X-Amz-Target` header value.
///
/// Formats seen in the wild:
/// - `"DynamoDB_20120810.GetItem"`
/// - `"AmazonSQS.CreateQueue"`
/// - `"AWSSecurityTokenServiceV20110615.GetCallerIdentity"`
///
/// Uses `eq_ignore_ascii_case` matching throughout — no heap allocation.
fn service_from_target(target: &str) -> Option<&'static str> {
    // Take everything before the first '.' then before the first '_'.
    let prefix = target.split('.').next().unwrap_or(target);
    let prefix = prefix.split('_').next().unwrap_or(prefix);

    // Strip trailing version suffixes like "V20110615" or "v20120810".
    let prefix = if prefix.len() > 9
        && (prefix[prefix.len() - 9..].eq_ignore_ascii_case("v20110615")
            || prefix[prefix.len() - 9..].eq_ignore_ascii_case("v20120810"))
    {
        &prefix[..prefix.len() - 9]
    } else {
        prefix
    };

    if prefix.eq_ignore_ascii_case("dynamodb") {
        Some("dynamodb")
    } else if prefix.eq_ignore_ascii_case("kinesis") {
        Some("kinesis")
    } else if prefix.eq_ignore_ascii_case("firehose") {
        Some("firehose")
    } else if prefix.eq_ignore_ascii_case("lambda") {
        Some("lambda")
    } else if prefix.eq_ignore_ascii_case("logs") {
        Some("logs")
    } else if prefix.eq_ignore_ascii_case("kms") {
        Some("kms")
    } else if prefix.eq_ignore_ascii_case("secretsmanager") {
        Some("secretsmanager")
    } else if prefix.eq_ignore_ascii_case("ssm") {
        Some("ssm")
    } else if prefix.eq_ignore_ascii_case("cloudwatch") {
        Some("cloudwatch")
    } else if prefix.eq_ignore_ascii_case("awsevents") || prefix.eq_ignore_ascii_case("events") {
        Some("events")
    } else if prefix.eq_ignore_ascii_case("amazonec2containerregistry")
        || prefix.eq_ignore_ascii_case("ecr")
    {
        Some("ecr")
    } else if prefix.eq_ignore_ascii_case("sns") {
        Some("sns")
    } else if prefix.eq_ignore_ascii_case("amazonsqs") || prefix.eq_ignore_ascii_case("sqs") {
        Some("sqs")
    } else if prefix.eq_ignore_ascii_case("awssecuritytokenservice")
        || prefix.eq_ignore_ascii_case("sts")
    {
        Some("sts")
    } else {
        None
    }
}

fn service_from_path(path: &str) -> Option<&'static str> {
    // Common path-based routing
    let path = path.trim_start_matches('/');
    if path.starts_with("2015-03-31/functions")
        || path.starts_with("2015-03-31/event-source-mappings")
    {
        return Some("lambda");
    }
    if path.starts_with("2012-12-01/") || path.contains("elasticloadbalancing") {
        return Some("elb");
    }
    // Default: can't determine from path alone
    None
}

fn parse_operation_and_params(
    method: &Method,
    path: &str,
    query_params: &HashMap<String, String>,
    headers: &HeaderMap,
    body: &Bytes,
    protocol: &AwsProtocol,
) -> Result<(String, serde_json::Value), String> {
    match protocol {
        AwsProtocol::Query => match parse_query_request(body) {
            Ok((op, params)) => Ok((op, params)),
            Err(e) => {
                let missing_action = e.to_string().contains("Missing 'Action' parameter");
                let query_mode = headers
                    .get("x-amzn-query-mode")
                    .and_then(|v| v.to_str().ok())
                    .map(|v| v == "true")
                    .unwrap_or(false);
                if missing_action && query_mode {
                    let target = headers
                        .get("x-amz-target")
                        .and_then(|v| v.to_str().ok())
                        .ok_or_else(|| e.to_string())?;
                    let operation = target
                        .split('.')
                        .nth(1)
                        .filter(|s| !s.is_empty())
                        .ok_or_else(|| e.to_string())?
                        .to_string();
                    let params = if body.is_empty() {
                        serde_json::json!({})
                    } else {
                        serde_json::from_slice(body).map_err(|err| err.to_string())?
                    };
                    Ok((operation, params))
                } else {
                    Err(e.to_string())
                }
            }
        },
        AwsProtocol::Ec2 => {
            let (op, params) = parse_ec2_request(body).map_err(|e| e.to_string())?;
            Ok((op, params))
        }
        AwsProtocol::Json => {
            let target = headers.get("x-amz-target").and_then(|v| v.to_str().ok());
            let (op, params) = parse_json_request(body, target).map_err(|e| e.to_string())?;
            Ok((op, params))
        }
        AwsProtocol::RestJson => {
            let params = parse_rest_json_request(method.as_str(), path, body, query_params)
                .map_err(|e| e.to_string())?;
            // For REST-JSON, operation comes from path routing (service-specific)
            let op = extract_rest_operation(method.as_str(), path, &params);
            Ok((op, params))
        }
        AwsProtocol::RestXml => {
            let params = parse_rest_xml_request(method.as_str(), path, body, query_params)
                .map_err(|e| e.to_string())?;
            let op = extract_rest_operation(method.as_str(), path, &params);
            Ok((op, params))
        }
    }
}

/// Extract operation name from REST path + method.
/// The actual operation mapping is done per-service in the provider.
fn extract_rest_operation(method: &str, path: &str, _params: &serde_json::Value) -> String {
    if path == "/2021-01-01/opensearch/domain" {
        return match method {
            "POST" => "CreateDomain".to_string(),
            "GET" => "ListDomainNames".to_string(),
            _ => format!("{}:{}", method, path),
        };
    }
    if path == "/2021-01-01/domain" {
        return match method {
            "GET" => "ListDomainNames".to_string(),
            _ => format!("{}:{}", method, path),
        };
    }
    if path.starts_with("/2021-01-01/opensearch/domain/") {
        return match method {
            "GET" => "DescribeDomain".to_string(),
            "DELETE" => "DeleteDomain".to_string(),
            _ => format!("{}:{}", method, path),
        };
    }
    if path == "/2013-04-01/hostedzone" {
        return match method {
            "POST" => "CreateHostedZone".to_string(),
            "GET" => "ListHostedZones".to_string(),
            _ => format!("{}:{}", method, path),
        };
    }
    if path.starts_with("/2013-04-01/hostedzone/") {
        if path.ends_with("/rrset") {
            return match method {
                "GET" => "ListResourceRecordSets".to_string(),
                "POST" => "ChangeResourceRecordSets".to_string(),
                _ => format!("{}:{}", method, path),
            };
        }
        return match method {
            "DELETE" => "DeleteHostedZone".to_string(),
            _ => format!("{}:{}", method, path),
        };
    }
    if path.starts_with("/2015-03-31/functions/") {
        let suffix = path.trim_start_matches("/2015-03-31/functions/");
        if suffix.ends_with("/code") && method == "PUT" {
            return "UpdateFunctionCode".to_string();
        }
        if suffix.ends_with("/invocations") && method == "POST" {
            return "Invoke".to_string();
        }
        if method == "GET" {
            return "GetFunction".to_string();
        }
        if method == "DELETE" {
            return "DeleteFunction".to_string();
        }
        if method == "PUT" {
            return "UpdateFunctionConfiguration".to_string();
        }
    }
    if path == "/2015-03-31/functions/" && method == "GET" {
        return "ListFunctions".to_string();
    }
    // For REST protocols, the operation is inferred by the service provider
    // We store method + path in the params for the provider to use
    format!("{}:{}", method, path)
}

/// Handle internal API requests (/_localstack/*)
async fn handle_internal_api(
    path: String,
    method: &Method,
    _headers: &HeaderMap,
    _query_params: &HashMap<String, String>,
    _body: &Bytes,
    _state: &AppState,
) -> Response {
    if is_studio_guided_execution_route(&path) {
        if *method != Method::POST {
            return (
                StatusCode::METHOD_NOT_ALLOWED,
                "method not allowed for guided execution endpoint",
            )
                .into_response();
        }
        if _body.len() > STUDIO_GUIDED_MAX_PAYLOAD_BYTES {
            return (
                StatusCode::PAYLOAD_TOO_LARGE,
                "guided execution payload exceeds configured limit",
            )
                .into_response();
        }
    }

    let uri = if _query_params.is_empty() {
        path
    } else {
        let query = serde_urlencoded::to_string(_query_params).unwrap_or_default();
        format!("{}?{}", path, query)
    };

    let mut req_builder = axum::http::Request::builder()
        .method(method.clone())
        .uri(uri);
    // HeaderMap iter yields (&HeaderName, &HeaderValue) — copy directly, no string round-trip.
    for (k, v) in _headers {
        req_builder = req_builder.header(k, v);
    }
    let req = req_builder
        .body(Body::from(_body.clone()))
        .unwrap_or_default();

    use tower::ServiceExt;
    match _state.internal_api_router.clone().oneshot(req).await {
        Ok(resp) => resp,
        Err(e) => {
            error!(error = %e, "internal API router dispatch failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "Internal API error").into_response()
        }
    }
}

fn is_studio_guided_execution_route(path: &str) -> bool {
    path == "/_localstack/studio-api/flows/execute"
        || path == "/_localstack/studio-api/flows/replay"
}

/// Returns `true` for S3 requests where the body is raw object data
/// (binary), not XML/JSON.  For these requests we skip materializing the
/// body into a `Bytes` heap allocation and let the S3 provider stream it
/// directly from the `SpooledBody`.
///
/// The heuristic: the Authorization header identifies the service as "s3",
/// the method is PUT or POST with a non-empty key segment in the path.
/// Content-Type is NOT checked because the S3 SDK may omit it or set it
/// to "application/octet-stream" for arbitrary object data.
fn is_s3_object_body_request(
    method: &Method,
    path: &str,
    _headers: &HeaderMap,
    query_params: &HashMap<String, String>,
) -> bool {
    // Must be PUT or POST (GET, HEAD, DELETE have no object body).
    if method != Method::PUT && method != Method::POST {
        return false;
    }

    // Path must have at least two non-empty segments: /bucket/key
    // (bucket-level PUT operations like ?versioning, ?policy have no key).
    //
    // S3 path-style URLs have a plain /bucket/key structure.  No other
    // service uses this pattern for binary object uploads:
    //   - Lambda paths start with 2015-03-31/
    //   - SQS/SNS/DynamoDB POST to the root (/) or /?QueueUrl= style
    //   - ELB paths start with 2012-12-01/
    //
    // We intentionally do NOT require an Authorization header here so that
    // benchmark and integration clients that omit auth (e.g. curl/oha without
    // signing) still get the streaming path and never load binary object
    // bodies into heap memory.
    let path_no_query = path.split('?').next().unwrap_or(path);
    let segments: Vec<&str> = path_no_query
        .trim_start_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();

    if segments.len() < 2 {
        return false;
    }

    // Exclude paths that belong to known non-S3 services (versioned REST APIs).
    let first = segments[0];
    // Lambda, ELB, EC2 (rare REST calls)
    if first.starts_with("2015-03-31")
        || first.starts_with("2012-12-01")
        || first.starts_with("2016-11-15")
    {
        return false;
    }

    // Exclude POST sub-resource operations that carry XML bodies, not binary
    // object data.  These are identified by specific query params:
    //
    //  - CompleteMultipartUpload: POST /bucket/key?uploadId=<id>  (no partNumber)
    //  - DeleteObjects:           POST /bucket?delete             (single-segment path)
    //
    // UploadPart is a POST with BOTH ?uploadId= AND ?partNumber= — that IS a
    // binary body and must NOT be excluded.
    if method == Method::POST {
        let has_upload_id = query_params.contains_key("uploadId");
        let has_part_number = query_params.contains_key("partNumber");
        if has_upload_id && !has_part_number {
            // CompleteMultipartUpload — XML body, not binary.
            return false;
        }
        if !has_upload_id && !has_part_number {
            // Any other key-level POST without multipart params (e.g. restore-object)
            // may also carry XML.  Be conservative and only treat UploadPart as binary.
            return false;
        }
    }

    true
}
