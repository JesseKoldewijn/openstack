use std::collections::HashMap;

use crate::api::{RawRequest, RawResponse};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawConsoleState {
    pub method: String,
    pub path: String,
    pub query: HashMap<String, String>,
    pub headers: HashMap<String, String>,
    pub body: Option<String>,
    pub last_response: Option<RawResponse>,
}

impl Default for RawConsoleState {
    fn default() -> Self {
        Self {
            method: "GET".to_string(),
            path: "/_localstack/health".to_string(),
            query: HashMap::new(),
            headers: HashMap::new(),
            body: None,
            last_response: None,
        }
    }
}

impl RawConsoleState {
    pub fn to_request(&self) -> RawRequest {
        RawRequest {
            method: self.method.clone(),
            path: self.path.clone(),
            query: self.query.clone(),
            headers: self.headers.clone(),
            body: self.body.clone(),
        }
    }

    pub fn apply_response(&mut self, response: RawResponse) {
        self.last_response = Some(response);
    }

    pub fn apply_request(&mut self, request: &RawRequest) {
        self.method = request.method.clone();
        self.path = request.path.clone();
        self.query = request.query.clone();
        self.headers = request.headers.clone();
        self.body = request.body.clone();
    }
}
