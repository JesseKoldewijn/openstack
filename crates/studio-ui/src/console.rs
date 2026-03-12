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
    /// Creates a RawConsoleState initialized to a default GET health-check request.
    ///
    /// The default state has `method` set to `"GET"`, `path` set to `"/_localstack/health"`,
    /// empty `query` and `headers`, and `body` and `last_response` set to `None`.
    ///
    /// # Examples
    ///
    /// ```
    /// let s = RawConsoleState::default();
    /// assert_eq!(s.method, "GET");
    /// assert_eq!(s.path, "/_localstack/health");
    /// assert!(s.query.is_empty());
    /// assert!(s.headers.is_empty());
    /// assert_eq!(s.body, None);
    /// assert_eq!(s.last_response, None);
    /// ```
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
    /// Builds a `RawRequest` from the console state's current fields.
    ///
    /// # Returns
    ///
    /// A `RawRequest` containing clones of the state's `method`, `path`, `query`, `headers`, and `body`.
    ///
    /// # Examples
    ///
    /// ```
    /// let state = RawConsoleState::default();
    /// let req = state.to_request();
    /// assert_eq!(req.method, state.method);
    /// assert_eq!(req.path, state.path);
    /// assert_eq!(req.query, state.query);
    /// assert_eq!(req.headers, state.headers);
    /// assert_eq!(req.body, state.body);
    /// ```
    pub fn to_request(&self) -> RawRequest {
        RawRequest {
            method: self.method.clone(),
            path: self.path.clone(),
            query: self.query.clone(),
            headers: self.headers.clone(),
            body: self.body.clone(),
        }
    }

    /// Record the given response as this console state's last response.
    pub fn apply_response(&mut self, response: RawResponse) {
        self.last_response = Some(response);
    }

    /// Update the console state's request fields to match the provided `RawRequest`.
    ///
    /// Copies the request fields (method, path, query, headers, body) into `self`,
    /// replacing the console state's current values.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::collections::HashMap;
    /// use crate::api::RawRequest;
    /// use crate::console::RawConsoleState;
    ///
    /// let mut state = RawConsoleState::default();
    /// let req = RawRequest {
    ///     method: "POST".into(),
    ///     path: "/items".into(),
    ///     query: {
    ///         let mut q = HashMap::new();
    ///         q.insert("q".into(), "1".into());
    ///         q
    ///     },
    ///     headers: {
    ///         let mut h = HashMap::new();
    ///         h.insert("content-type".into(), "application/json".into());
    ///         h
    ///     },
    ///     body: Some("{\"name\":\"x\"}".into()),
    /// };
    ///
    /// state.apply_request(&req);
    /// assert_eq!(state.method, "POST");
    /// assert_eq!(state.path, "/items");
    /// assert_eq!(state.body.as_deref(), Some("{\"name\":\"x\"}"));
    /// ```
    pub fn apply_request(&mut self, request: &RawRequest) {
        self.method = request.method.clone();
        self.path = request.path.clone();
        self.query = request.query.clone();
        self.headers = request.headers.clone();
        self.body = request.body.clone();
    }
}
