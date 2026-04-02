use std::collections::HashMap;

use serde::Deserialize;
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
    #[error("invalid raw request url: {0}")]
    InvalidRawUrl(String),
    #[error("invalid raw request method: {0}")]
    InvalidRawMethod(String),
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

// ---------------------------------------------------------------------------
// Runtime config
// ---------------------------------------------------------------------------

/// Credentials and endpoint returned by `/_localstack/studio-api/runtime-config`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct StudioCredentials {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: Option<String>,
}

/// Polling intervals from runtime-config.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct StudioPollingConfig {
    pub storage_interval_ms: u64,
    pub transactions_interval_ms: u64,
}

/// Full response from `/_localstack/studio-api/runtime-config`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct StudioRuntimeConfig {
    pub schema_version: String,
    pub endpoint: String,
    pub credentials: StudioCredentials,
    pub region: String,
    pub polling: StudioPollingConfig,
}

// ---------------------------------------------------------------------------
// Operations catalogue response
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ServiceOperationsResponse {
    pub service: String,
    pub total: usize,
    pub guided_count: usize,
    pub operations: Vec<OperationEntryDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct OperationEntryDto {
    pub name: String,
    pub method: String,
    pub path: String,
    pub has_guided_flow: bool,
}

// ---------------------------------------------------------------------------
// Storage snapshot response
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ServiceStorageResponse {
    pub service: String,
    pub snapshot: Option<Value>,
}

// ---------------------------------------------------------------------------
// Transaction response
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TransactionDto {
    pub id: u64,
    pub service: String,
    pub operation: Option<String>,
    pub method: String,
    pub path: String,
    pub status: u16,
    pub outcome: String,
    pub duration_ms: Option<u64>,
    pub started_at_ms: u64,
    pub from_guided_flow: bool,
    pub request_body_preview: Option<String>,
    pub response_body_preview: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TransactionSummaryDto {
    pub total: usize,
    pub success: usize,
    pub client_error: usize,
    pub server_error: usize,
    pub pending: usize,
    pub avg_duration_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ServiceTransactionsResponse {
    pub service: String,
    pub total: usize,
    pub transactions: Vec<TransactionDto>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct AllTransactionsResponse {
    pub schema_version: String,
    pub summary: TransactionSummaryDto,
    pub transactions: Vec<TransactionDto>,
}

// ---------------------------------------------------------------------------
// StudioUrlResolution helpers
// ---------------------------------------------------------------------------

pub async fn resolve_studio_url(
    explicit_url: Option<&str>,
    daemon_health_url: Option<&str>,
    fallback_base_url: &str,
) -> StudioUrlResolution {
    resolve_studio_url_with_timeout(explicit_url, daemon_health_url, fallback_base_url, 600).await
}

pub async fn resolve_studio_url_with_timeout(
    explicit_url: Option<&str>,
    daemon_health_url: Option<&str>,
    fallback_base_url: &str,
    timeout_ms: u64,
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
        .timeout(std::time::Duration::from_millis(timeout_ms))
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

fn strip_health_suffix(url: &str) -> String {
    url.strip_suffix("/_localstack/health")
        .unwrap_or(url)
        .to_string()
}

// ---------------------------------------------------------------------------
// StudioApiClient
// ---------------------------------------------------------------------------

impl StudioApiClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            base_url: base_url.into(),
            http,
        }
    }

    // ── Existing endpoints ───────────────────────────────────────────────

    pub async fn services(&self) -> Result<StudioServicesResponse, StudioApiError> {
        let url = format!("{}/_localstack/studio-api/services", self.base_url);
        Ok(self
            .http
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    pub async fn interaction_schema(&self) -> Result<InteractionSchema, StudioApiError> {
        let url = format!(
            "{}/_localstack/studio-api/interactions/schema",
            self.base_url
        );
        Ok(self
            .http
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    pub async fn flow_catalog(&self) -> Result<FlowCatalogResponse, StudioApiError> {
        let url = format!("{}/_localstack/studio-api/flows/catalog", self.base_url);
        Ok(self
            .http
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    pub async fn flow_definition(
        &self,
        service: &str,
    ) -> Result<FlowDefinitionResponse, StudioApiError> {
        let url = format!("{}/_localstack/studio-api/flows/{}", self.base_url, service);
        Ok(self
            .http
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    pub async fn flow_coverage(&self) -> Result<FlowCoverageResponse, StudioApiError> {
        let url = format!("{}/_localstack/studio-api/flows/coverage", self.base_url);
        Ok(self
            .http
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    // ── New endpoints ────────────────────────────────────────────────────

    /// Fetch the Studio runtime config (credentials, endpoint, polling).
    pub async fn runtime_config(&self) -> Result<StudioRuntimeConfig, StudioApiError> {
        let url = format!("{}/_localstack/studio-api/runtime-config", self.base_url);
        Ok(self
            .http
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    /// Fetch the operation catalogue for a single service.
    pub async fn service_operations(
        &self,
        service: &str,
    ) -> Result<ServiceOperationsResponse, StudioApiError> {
        let url = format!(
            "{}/_localstack/studio-api/operations/{}",
            self.base_url, service
        );
        Ok(self
            .http
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    /// Fetch the operation catalogue for all services.
    pub async fn all_operations(&self) -> Result<Value, StudioApiError> {
        let url = format!("{}/_localstack/studio-api/operations", self.base_url);
        Ok(self
            .http
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    /// Fetch the live storage snapshot for a single service.
    pub async fn service_storage(
        &self,
        service: &str,
    ) -> Result<ServiceStorageResponse, StudioApiError> {
        let url = format!(
            "{}/_localstack/studio-api/storage/{}",
            self.base_url, service
        );
        Ok(self
            .http
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    /// Fetch the transaction log for a service, with optional filters.
    pub async fn service_transactions(
        &self,
        service: &str,
        outcome: Option<&str>,
        guided_only: bool,
        limit: usize,
    ) -> Result<ServiceTransactionsResponse, StudioApiError> {
        let mut url = format!(
            "{}/_localstack/studio-api/transactions/{}?limit={}",
            self.base_url, service, limit
        );
        if let Some(o) = outcome {
            url.push_str(&format!("&outcome={}", o));
        }
        if guided_only {
            url.push_str("&guided_only=true");
        }
        Ok(self
            .http
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    /// Clear the transaction log.
    pub async fn clear_transactions(&self) -> Result<(), StudioApiError> {
        let url = format!("{}/_localstack/studio-api/transactions", self.base_url);
        self.http.delete(url).send().await?.error_for_status()?;
        Ok(())
    }

    /// Record a completed transaction (typically called from the SPA after an operation).
    pub async fn record_transaction(&self, body: &serde_json::Value) -> Result<(), StudioApiError> {
        let url = format!(
            "{}/_localstack/studio-api/transactions/record",
            self.base_url
        );
        self.http
            .post(url)
            .json(body)
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    // ── Raw request execution ─────────────────────────────────────────────

    pub async fn execute_raw(&self, request: &RawRequest) -> Result<RawResponse, StudioApiError> {
        let mut url = reqwest::Url::parse(&format!("{}{}", self.base_url, request.path))
            .map_err(|e| StudioApiError::InvalidRawUrl(e.to_string()))?;
        {
            let mut qp = url.query_pairs_mut();
            for (k, v) in &request.query {
                qp.append_pair(k, v);
            }
        }

        let method = reqwest::Method::from_bytes(request.method.as_bytes())
            .map_err(|_| StudioApiError::InvalidRawMethod(request.method.clone()))?;
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
