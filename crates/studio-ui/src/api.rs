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

impl StudioApiClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            http: reqwest::Client::new(),
        }
    }

    pub async fn services(&self) -> Result<StudioServicesResponse, StudioApiError> {
        let url = format!("{}/_localstack/studio-api/services", self.base_url);
        Ok(self.http.get(url).send().await?.json().await?)
    }

    pub async fn interaction_schema(&self) -> Result<InteractionSchema, StudioApiError> {
        let url = format!(
            "{}/_localstack/studio-api/interactions/schema",
            self.base_url
        );
        Ok(self.http.get(url).send().await?.json().await?)
    }

    pub async fn flow_catalog(&self) -> Result<FlowCatalogResponse, StudioApiError> {
        let url = format!("{}/_localstack/studio-api/flows/catalog", self.base_url);
        Ok(self.http.get(url).send().await?.json().await?)
    }

    pub async fn flow_definition(
        &self,
        service: &str,
    ) -> Result<FlowDefinitionResponse, StudioApiError> {
        let url = format!("{}/_localstack/studio-api/flows/{}", self.base_url, service);
        Ok(self.http.get(url).send().await?.json().await?)
    }

    pub async fn flow_coverage(&self) -> Result<FlowCoverageResponse, StudioApiError> {
        let url = format!("{}/_localstack/studio-api/flows/coverage", self.base_url);
        Ok(self.http.get(url).send().await?.json().await?)
    }

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
