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

fn strip_health_suffix(url: &str) -> String {
    url.strip_suffix("/_localstack/health")
        .unwrap_or(url)
        .to_string()
}

impl StudioApiClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            http: reqwest::Client::new(),
        }
    }

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
