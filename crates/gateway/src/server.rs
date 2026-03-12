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
use tower::ServiceBuilder;
use tower_http::compression::CompressionLayer;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::context::RequestContext;
use crate::cors::CorsHandler;
use crate::sigv4::{
    DEFAULT_ACCESS_KEY, DEFAULT_REGION, access_key_to_account_id, is_valid_region, parse_sigv4_auth,
};

const STUDIO_SPA: &str = r#"<!doctype html>
<html lang=\"en\">
<head>
  <meta charset=\"utf-8\" />
  <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\" />
  <title>openstack studio</title>
  <style>
    :root { color-scheme: light dark; }
    body { margin:0; font-family: ui-sans-serif,system-ui,sans-serif; }
    main { padding: 24px; max-width: 900px; margin: 0 auto; }
    h1 { margin: 0 0 12px; }
    p { opacity: 0.9; }
    code { background: #00000022; padding: 2px 6px; border-radius: 6px; }
  </style>
</head>
<body>
  <main>
    <h1>openstack studio</h1>
    <p>Studio shell is available. Frontend bundle integration is pending.</p>
    <p>API: <code>/_localstack/studio-api/services</code></p>
  </main>
</body>
</html>
"#;

const STUDIO_ASSET_JS: &str = "console.log('openstack studio shell');";

const STUDIO_ASSET_CSS: &str = ":root{color-scheme:light dark}body{margin:0}";
const STUDIO_GUIDED_MAX_PAYLOAD_BYTES: usize = 256 * 1024;

/// Adapter that converts `http_body_util::BodyStream<axum::body::Body>` into
/// a `futures_core::Stream<Item = Result<Bytes, io::Error>>` suitable for
/// `SpooledBody::write_from_stream()`.
struct BodyStreamAdapter {
    inner: BodyStream<Body>,
}

impl BodyStreamAdapter {
    fn new(body: Body) -> Self {
        Self {
            inner: BodyStream::new(body),
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
}

/// Shared application state passed to all axum handlers.
#[derive(Clone)]
struct AppState {
    config: Config,
    plugin_manager: ServicePluginManager,
    cors: Arc<CorsHandler>,
}

impl Gateway {
    pub fn new(config: Config, plugin_manager: ServicePluginManager) -> Self {
        Self {
            config,
            plugin_manager,
        }
    }

    /// Build the axum Router for this gateway (useful for testing).
    fn build_app(&self) -> Router {
        let cors = Arc::new(CorsHandler::new(&self.config));
        let app_state = AppState {
            config: self.config.clone(),
            plugin_manager: self.plugin_manager.clone(),
            cors,
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
        tokio::select! {
            result = axum::serve(listener, app) => { result?; }
            _ = &mut shutdown => {
                info!("Shutdown signal received");
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
        tokio::signal::ctrl_c().await?;
        info!("Shutdown signal received");

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
    let request_id = Uuid::new_v4().to_string();

    // Extract path and query string
    let uri = req.uri().clone();
    let path = uri.path().to_string();
    let query_string = uri.query().unwrap_or("").to_string();

    // Parse query parameters
    let query_params: HashMap<String, String> = if query_string.is_empty() {
        HashMap::new()
    } else {
        serde_urlencoded::from_str(&query_string).unwrap_or_default()
    };

    // Collect headers into a HashMap (lowercase keys)
    let mut header_map: HashMap<String, String> = HashMap::with_capacity(headers.len());
    for (k, v) in &headers {
        if let Ok(vs) = v.to_str() {
            header_map.insert(k.as_str().to_ascii_lowercase(), vs.to_string());
        }
    }

    // Stream the request body into a SpooledBody
    let threshold = state.config.body_spool_threshold_bytes;
    let mut spooled = SpooledBody::new(threshold);
    let stream = BodyStreamAdapter::new(req.into_body());
    if let Err(e) = spooled.write_from_stream(stream).await {
        error!("Failed to read request body: {}", e);
        return (StatusCode::BAD_REQUEST, "Failed to read request body").into_response();
    }

    // Materialize raw_body as Bytes for protocol parsing
    let body_bytes = match spooled.to_bytes() {
        Ok(b) => b,
        Err(e) => {
            error!("Failed to materialize request body: {}", e);
            return (StatusCode::BAD_REQUEST, "Failed to read request body").into_response();
        }
    };

    // Handle CORS preflight
    if CorsHandler::is_preflight(&method, &headers) {
        let mut resp_headers = HeaderMap::new();
        state.cors.add_cors_headers(
            &mut resp_headers,
            header_map.get("origin").map(|s| s.as_str()),
        );
        let mut response = StatusCode::OK.into_response();
        *response.headers_mut() = resp_headers;
        return response;
    }

    // Studio SPA routes are resolved before generic AWS inference.
    if is_studio_asset_route(&path) {
        return studio_asset_response(&path);
    }

    if is_studio_spa_route(&path) {
        return studio_spa_response();
    }

    // Internal API routes go to the internal API handler.
    if path.starts_with("/_localstack/") {
        return handle_internal_api(
            path,
            &method,
            &header_map,
            &query_params,
            &body_bytes,
            &state,
        )
        .await;
    }

    // Build request context
    let context_start = std::time::Instant::now();
    let ctx = match build_request_context(
        &method,
        &path,
        &query_params,
        &header_map,
        &body_bytes,
        &request_id,
        &state.config,
        spooled,
    ) {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };
    let context_latency_us = context_start.elapsed().as_micros();

    let service = ctx.service.clone();
    let operation = ctx.operation.clone();
    let region = ctx.region.clone();
    let account_id = ctx.account_id.clone();
    let protocol = ctx.protocol.clone();

    debug!(
        request_id = %request_id,
        service = %service,
        operation = %operation,
        region = %region,
        account_id = %account_id,
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
                service = %service,
                operation = %operation,
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
                    500,
                ),
                DispatchError::ServiceUnavailable(msg) => ("ServiceUnavailable", msg.clone(), 503),
                DispatchError::ProviderError(msg) => ("InternalFailure", msg.clone(), 500),
                DispatchError::SerializationError(msg) => ("InternalFailure", msg.clone(), 500),
            };

            warn!(
                request_id = %request_id,
                service = %service,
                operation = %operation,
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
                ct.to_string(),
                Vec::new(),
            )
        }
    };

    // Build the response based on body variant
    let mut response = match resp_body {
        ResponseBody::Buffered(bytes) => Response::builder()
            .status(status)
            .header("content-type", &content_type)
            .header("x-amzn-requestid", &request_id)
            .body(Body::from(bytes))
            .unwrap_or_default(),
        ResponseBody::Streaming {
            stream,
            content_length,
        } => {
            let mut builder = Response::builder()
                .status(status)
                .header("content-type", &content_type)
                .header("x-amzn-requestid", &request_id);
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
    state.cors.add_cors_headers(
        response.headers_mut(),
        header_map.get("origin").map(|s| s.as_str()),
    );

    response
}

fn is_studio_spa_route(path: &str) -> bool {
    path == "/_localstack/studio"
        || path == "/_localstack/studio/"
        || path.starts_with("/_localstack/studio/")
}

fn is_studio_asset_route(path: &str) -> bool {
    path == "/_localstack/studio/assets/app.js" || path == "/_localstack/studio/assets/app.css"
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
    let (content_type, body) = match path {
        "/_localstack/studio/assets/app.js" => {
            ("application/javascript; charset=utf-8", STUDIO_ASSET_JS)
        }
        "/_localstack/studio/assets/app.css" => ("text/css; charset=utf-8", STUDIO_ASSET_CSS),
        _ => ("text/plain; charset=utf-8", "Not found"),
    };

    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", content_type)
        .header("cache-control", "public, max-age=31536000, immutable")
        .header("etag", "\"studio-asset-v1\"")
        .body(Body::from(body))
        .unwrap_or_default()
}

#[allow(clippy::result_large_err)]
fn build_request_context(
    method: &Method,
    path: &str,
    query_params: &HashMap<String, String>,
    headers: &HashMap<String, String>,
    body: &Bytes,
    request_id: &str,
    config: &Config,
    spooled_body: SpooledBody,
) -> Result<RequestContext, Response> {
    // Parse SigV4 Authorization or inject default
    let (access_key, region, service_from_auth) = if let Some(auth) = headers.get("authorization") {
        if let Some(sigv4) = parse_sigv4_auth(auth) {
            (sigv4.access_key, sigv4.region, Some(sigv4.service))
        } else {
            (
                DEFAULT_ACCESS_KEY.to_string(),
                DEFAULT_REGION.to_string(),
                None,
            )
        }
    } else {
        (
            DEFAULT_ACCESS_KEY.to_string(),
            DEFAULT_REGION.to_string(),
            None,
        )
    };

    // Derive account ID from access key
    let account_id = access_key_to_account_id(&access_key);

    // Determine the target service
    let service = detect_service(
        path,
        query_params,
        headers,
        body,
        service_from_auth.as_deref(),
    );

    // Validate / normalize region
    let region = if config.allow_nonstandard_regions || is_valid_region(&region) {
        region
    } else {
        warn!("Invalid region '{}', falling back to us-east-1", region);
        DEFAULT_REGION.to_string()
    };

    // Determine the protocol used by this service
    let protocol = AwsProtocol::from_service(&service);

    // Parse the request body according to protocol
    let (operation, params) =
        match parse_operation_and_params(method, path, query_params, headers, body, &protocol) {
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
        access_key,
        protocol,
        params,
        raw_body: body.clone(),
        headers: headers.clone(),
        path: path.to_string(),
        method: method.to_string(),
        query_params: query_params.clone(),
        request_id: request_id.to_string(),
        spooled_body: Some(spooled_body),
    })
}

/// Detect which AWS service is being targeted.
fn detect_service(
    path: &str,
    query_params: &HashMap<String, String>,
    headers: &HashMap<String, String>,
    body: &Bytes,
    service_from_auth: Option<&str>,
) -> String {
    // 1. Authorization header credential scope (highest priority)
    if let Some(svc) = service_from_auth {
        return svc.to_lowercase();
    }

    // 2. Host header: sqs.us-east-1.localhost.localstack.cloud
    if let Some(host) = headers.get("host") {
        let host = host.split(':').next().unwrap_or(host);
        let parts: Vec<&str> = host.split('.').collect();
        if parts.len() >= 2 {
            let potential_service = parts[0].to_lowercase();
            // Check if it looks like a service name (all lowercase letters/digits)
            if potential_service
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
                && is_known_service(&potential_service)
            {
                return potential_service;
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

    if let Some(target) = headers.get("x-amz-target")
        && let Some(svc) = service_from_target(target)
    {
        return svc;
    }

    // 4. Query protocol Action (POST form body or query string)
    if let Some(svc) = service_from_query_action(query_params, body) {
        return svc;
    }

    // 5. URL path patterns
    if let Some(svc) = service_from_path(path) {
        return svc;
    }

    // 6. S3 path-style heuristic for unsigned endpoint-url calls
    // Examples: PUT /my-bucket, GET /my-bucket/key
    let trimmed = path.trim_start_matches('/');
    if !trimmed.is_empty() {
        return "s3".to_string();
    }

    "unknown".to_string()
}

fn service_from_query_action(
    query_params: &HashMap<String, String>,
    body: &Bytes,
) -> Option<String> {
    let action = query_params.get("Action").cloned().or_else(|| {
        let params = serde_urlencoded::from_bytes::<Vec<(String, String)>>(body).ok()?;
        params
            .into_iter()
            .find_map(|(k, v)| if k == "Action" { Some(v) } else { None })
    })?;

    match action.as_str() {
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
        | "ChangeMessageVisibilityBatch" => Some("sqs".to_string()),
        // STS
        "GetCallerIdentity" | "AssumeRole" => Some("sts".to_string()),
        // SNS
        "CreateTopic" | "DeleteTopic" | "Publish" | "Subscribe" | "Unsubscribe" | "ListTopics"
        | "SetTopicAttributes" | "GetTopicAttributes" => Some("sns".to_string()),
        // IAM
        "CreateRole" | "DeleteRole" | "ListRoles" | "GetRole" | "CreateUser" | "DeleteUser"
        | "ListUsers" | "GetUser" => Some("iam".to_string()),
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

fn service_from_target(target: &str) -> Option<String> {
    // Formats seen in the wild:
    // - "DynamoDB_20120810.GetItem"
    // - "AmazonSQS.CreateQueue"
    // - "AWSSecurityTokenServiceV20110615.GetCallerIdentity"
    let raw_prefix = target
        .split('.')
        .next()
        .unwrap_or(target)
        .split('_')
        .next()
        .unwrap_or(target)
        .to_lowercase();

    let prefix = raw_prefix
        .trim_end_matches("v20110615")
        .trim_end_matches("v20120810")
        .to_string();

    Some(
        match prefix.as_str() {
            "dynamodb" => "dynamodb",
            "kinesis" => "kinesis",
            "firehose" => "firehose",
            "lambda" => "lambda",
            "logs" => "logs",
            "kms" => "kms",
            "secretsmanager" => "secretsmanager",
            "ssm" => "ssm",
            "cloudwatch" => "cloudwatch",
            "sns" => "sns",
            "amazonsqs" | "sqs" => "sqs",
            "awssecuritytokenservice" | "sts" => "sts",
            _ => return None,
        }
        .to_string(),
    )
}

fn service_from_path(path: &str) -> Option<String> {
    // Common path-based routing
    let path = path.trim_start_matches('/');
    if path.starts_with("2015-03-31/functions")
        || path.starts_with("2015-03-31/event-source-mappings")
    {
        return Some("lambda".to_string());
    }
    if path.starts_with("2012-12-01/") || path.contains("elasticloadbalancing") {
        return Some("elb".to_string());
    }
    // Default: can't determine from path alone
    None
}

fn parse_operation_and_params(
    method: &Method,
    path: &str,
    query_params: &HashMap<String, String>,
    headers: &HashMap<String, String>,
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
                    .map(|v| v == "true")
                    .unwrap_or(false);
                if missing_action && query_mode {
                    let target = headers.get("x-amz-target").ok_or_else(|| e.to_string())?;
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
            let target = headers.get("x-amz-target").map(|s| s.as_str());
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
    // For REST protocols, the operation is inferred by the service provider
    // We store method + path in the params for the provider to use
    format!("{}:{}", method, path)
}

/// Handle internal API requests (/_localstack/*)
async fn handle_internal_api(
    path: String,
    method: &Method,
    _headers: &HashMap<String, String>,
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

    let shutdown_tx = tokio::sync::broadcast::channel::<()>(1).0;
    let state = openstack_internal_api::ApiState::new(
        _state.config.clone(),
        _state.plugin_manager.clone(),
        shutdown_tx,
    );
    let router = openstack_internal_api::internal_api_router(state);

    let uri = if _query_params.is_empty() {
        path
    } else {
        let query = serde_urlencoded::to_string(_query_params).unwrap_or_default();
        format!("{}?{}", path, query)
    };

    let mut req_builder = axum::http::Request::builder()
        .method(method.clone())
        .uri(uri);
    for (k, v) in _headers {
        if let (Ok(name), Ok(value)) = (
            axum::http::header::HeaderName::from_bytes(k.as_bytes()),
            axum::http::header::HeaderValue::from_str(v),
        ) {
            req_builder = req_builder.header(name, value);
        }
    }
    let req = req_builder
        .body(Body::from(_body.clone()))
        .unwrap_or_default();

    use tower::ServiceExt;
    match router.oneshot(req).await {
        Ok(resp) => resp,
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Internal API error").into_response(),
    }
}

fn is_studio_guided_execution_route(path: &str) -> bool {
    path == "/_localstack/studio-api/flows/execute"
        || path == "/_localstack/studio-api/flows/replay"
}
