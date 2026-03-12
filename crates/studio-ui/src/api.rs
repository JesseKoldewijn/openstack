use std::collections::HashMap;

use serde_json::Value;
use thiserror::Error;

use crate::models::{
    FlowCatalogResponse, FlowCoverageResponse, FlowDefinitionResponse, InteractionSchema,
    StudioServicesResponse,
};

#[derive(Debug, Clone)]
pub struct StudioApiClient {
    base_url: String,
    http: reqwest::Client,
}

#[derive(Debug, Error)]
pub enum StudioApiError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawRequest {
    pub method: String,
    pub path: String,
    pub query: HashMap<String, String>,
    pub headers: HashMap<String, String>,
    pub body: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StudioUrlResolution {
    pub url: String,
    pub source: String,
    pub daemon_ready: bool,
}

/// Determines the Studio base URL, its resolution source, and whether a local daemon is responding.
///
/// The resolution logic prefers, in order:
/// 1. `explicit_url` if provided (source = "explicit"),
/// 2. `daemon_health_url` with the `/ _localstack/health` suffix removed if present (source = "daemon"),
/// 3. otherwise `fallback_base_url` (source = "fallback").
/// The function then checks `base_url/_localstack/health` with a short timeout (600 ms) to set `daemon_ready`.
/// The returned `StudioUrlResolution.url` is `base_url/_localstack/studio`.
///
/// # Examples
///
/// ```
/// use std::collections::HashMap;
/// // Run the async function in a temporary runtime for demonstration.
/// let rt = tokio::runtime::Runtime::new().unwrap();
/// let res = rt.block_on(async {
///     resolve_studio_url(Some("http://example.com"), None, "http://fallback")
/// }).unwrap_or_else(|_| panic!());
/// assert!(res.url.ends_with("/_localstack/studio"));
/// assert_eq!(res.source, "explicit");
/// ```
pub async fn resolve_studio_url(
    explicit_url: Option<&str>,
    daemon_health_url: Option<&str>,
    fallback_base_url: &str,
) -> StudioUrlResolution {
    let (base_url, source) = if let Some(url) = explicit_url {
        (url.to_string(), "explicit")
    } else if let Some(url) = daemon_health_url {
        (strip_health_suffix(url), "daemon")
    } else {
        (fallback_base_url.to_string(), "fallback")
    };

    let health_url = format!("{}/_localstack/health", base_url.trim_end_matches('/'));
    let daemon_ready = if let Ok(client) = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(600))
        .build()
    {
        client
            .get(&health_url)
            .send()
            .await
            .map(|resp| resp.status().is_success())
            .unwrap_or(false)
    } else {
        false
    };

    StudioUrlResolution {
        url: format!("{}/_localstack/studio", base_url.trim_end_matches('/')),
        source: source.to_string(),
        daemon_ready,
    }
}

/// Removes the "/_localstack/health" suffix from a URL if present.
///
/// Returns the input string with the trailing "/_localstack/health" removed when present, otherwise returns the original string.
///
/// # Examples
///
/// ```
/// let s = strip_health_suffix("http://localhost:4566/_localstack/health");
/// assert_eq!(s, "http://localhost:4566");
///
/// let s2 = strip_health_suffix("http://example.com/_localstack/status");
/// assert_eq!(s2, "http://example.com/_localstack/status");
/// ```
fn strip_health_suffix(url: &str) -> String {
    url.strip_suffix("/_localstack/health")
        .unwrap_or(url)
        .to_string()
}

impl StudioApiClient {
    /// Creates a new StudioApiClient configured for the given base URL.
    ///
    /// The client is constructed with the provided `base_url` and a fresh `reqwest::Client` for HTTP requests.
    ///
    /// # Examples
    ///
    /// ```
    /// let client = crate::StudioApiClient::new("http://localhost:4566");
    /// assert!(client.base_url.starts_with("http://"));
    /// ```
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            http: reqwest::Client::new(),
        }
    }

    /// Fetches the list of Studio services from the configured Studio API base URL.
    ///
    /// # Returns
    ///
    /// `StudioServicesResponse` containing the services when the request and JSON deserialization succeed, or a `StudioApiError` on failure.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use crates::studio_ui::api::StudioApiClient;
    /// #[tokio::main]
    /// async fn main() {
    ///     let client = StudioApiClient::new("http://localhost:4566");
    ///     let services = client.services().await.unwrap();
    ///     println!("{:#?}", services);
    /// }
    /// ```
    pub async fn services(&self) -> Result<StudioServicesResponse, StudioApiError> {
        let url = format!("{}/_localstack/studio-api/services", self.base_url);
        Ok(self.http.get(url).send().await?.json().await?)
    }

    /// Fetches the interaction schema from the Studio API.
    ///
    /// Returns the deserialized `InteractionSchema` on success.
    ///
    /// # Examples
    ///
    /// ```
    /// let client = StudioApiClient::new("http://localhost:4566");
    /// let schema = tokio::runtime::Runtime::new().unwrap().block_on(async {
    ///     client.interaction_schema().await.unwrap()
    /// });
    /// ```
    pub async fn interaction_schema(&self) -> Result<InteractionSchema, StudioApiError> {
        let url = format!(
            "{}/_localstack/studio-api/interactions/schema",
            self.base_url
        );
        Ok(self.http.get(url).send().await?.json().await?)
    }

    /// Fetches the flow catalog from the Studio API.
    ///
    /// Returns the deserialized `FlowCatalogResponse` on success.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use crate::api::StudioApiClient;
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = StudioApiClient::new("http://localhost:4566");
    /// let catalog = client.flow_catalog().await?;
    /// // use `catalog`
    /// # Ok(()) }
    /// ```
    pub async fn flow_catalog(&self) -> Result<FlowCatalogResponse, StudioApiError> {
        let url = format!("{}/_localstack/studio-api/flows/catalog", self.base_url);
        Ok(self.http.get(url).send().await?.json().await?)
    }

    /// Fetches the flow definition for the specified service from the Studio API.
    ///
    /// Returns `Ok` with the deserialized `FlowDefinitionResponse` on success, or an `Err(StudioApiError)` if the HTTP request or JSON deserialization fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use crates::studio_ui::api::StudioApiClient;
    /// use tokio::runtime::Runtime;
    ///
    /// let rt = Runtime::new().unwrap();
    /// rt.block_on(async {
    ///     let client = StudioApiClient::new("http://localhost:4566");
    ///     let def = client.flow_definition("my-service").await.unwrap();
    ///     // use `def`
    /// });
    /// ```
    pub async fn flow_definition(
        &self,
        service: &str,
    ) -> Result<FlowDefinitionResponse, StudioApiError> {
        let url = format!("{}/_localstack/studio-api/flows/{}", self.base_url, service);
        Ok(self.http.get(url).send().await?.json().await?)
    }

    /// Fetches flow coverage information from the Studio API.
    ///
    /// # Returns
    ///
    /// A `FlowCoverageResponse` containing coverage details for flows.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use crates::studio_ui::api::StudioApiClient;
    /// # tokio_test::block_on(async {
    /// let client = StudioApiClient::new("http://localhost:4566");
    /// let coverage = client.flow_coverage().await.unwrap();
    /// // inspect fields on `coverage` as needed
    /// # });
    /// ```
    pub async fn flow_coverage(&self) -> Result<FlowCoverageResponse, StudioApiError> {
        let url = format!("{}/_localstack/studio-api/flows/coverage", self.base_url);
        Ok(self.http.get(url).send().await?.json().await?)
    }

    /// Executes a raw HTTP request described by `RawRequest` and returns a normalized `RawResponse`.
    ///
    /// The request URL is constructed by concatenating the client's `base_url` and `request.path`, then appending query parameters from `request.query`. Request headers and optional body are applied as provided. The response is read in full; if the body is valid JSON it is pretty-printed, otherwise the raw text is returned. The returned `RawResponse` contains the HTTP status code, response headers, and the (possibly pretty-printed) body.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::collections::HashMap;
    /// # use tokio_test::block_on;
    /// # async fn _example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = StudioApiClient::new("http://localhost:4566");
    /// let req = RawRequest {
    ///     method: "GET".into(),
    ///     path: "/_localstack/studio-api/ping".into(),
    ///     query: HashMap::new(),
    ///     headers: HashMap::new(),
    ///     body: None,
    /// };
    /// let resp = client.execute_raw(&req).await?;
    /// assert!(resp.status >= 100 && resp.status < 600);
    /// # Ok(()) };
    /// # block_on(_example()).unwrap();
    /// ```
    pub async fn execute_raw(&self, request: &RawRequest) -> Result<RawResponse, StudioApiError> {
        let mut url = reqwest::Url::parse(&format!("{}{}", self.base_url, request.path)).unwrap();
        {
            let mut qp = url.query_pairs_mut();
            for (k, v) in &request.query {
                qp.append_pair(k, v);
            }
        }

        let method = request.method.parse().unwrap_or(reqwest::Method::GET);
        let mut req = self.http.request(method, url);
        for (k, v) in &request.headers {
            req = req.header(k, v);
        }
        if let Some(body) = &request.body {
            req = req.body(body.clone());
        }

        let resp = req.send().await?;
        let status = resp.status().as_u16();
        let headers = resp
            .headers()
            .iter()
            .map(|(k, v)| {
                (
                    k.as_str().to_string(),
                    v.to_str().unwrap_or_default().to_string(),
                )
            })
            .collect::<HashMap<_, _>>();
        let raw_text = resp.text().await?;
        let body = match serde_json::from_str::<Value>(&raw_text) {
            Ok(v) => serde_json::to_string_pretty(&v).unwrap_or(raw_text),
            Err(_) => raw_text,
        };

        Ok(RawResponse {
            status,
            headers,
            body,
        })
    }
}
