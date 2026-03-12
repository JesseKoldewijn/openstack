#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DashboardRoute {
    Home,
    Service {
        service: String,
    },
    Replay {
        service: String,
        interaction_id: u64,
    },
}

impl DashboardRoute {
    /// Parses a route string into a `DashboardRoute`.
    ///
    /// Accepts canonical paths and common URL fragments and query forms, and maps them to:
    /// - `Home` for the empty or unrecognized path,
    /// - `Service { service }` for `/service/{service}`,
    /// - `Replay { service, interaction_id }` for `/service/{service}/replay/{interaction_id}` (falls back to `Home` if `interaction_id` is not a valid `u64`).
    ///
    /// # Examples
    ///
    /// ```
    /// use crate::navigation::DashboardRoute;
    ///
    /// assert_eq!(DashboardRoute::parse("/"), DashboardRoute::Home);
    /// assert_eq!(DashboardRoute::parse("#/service/foo"), DashboardRoute::Service { service: "foo".into() });
    /// assert_eq!(DashboardRoute::parse("/service/foo/replay/42"), DashboardRoute::Replay { service: "foo".into(), interaction_id: 42 });
    /// assert_eq!(DashboardRoute::parse("/service/foo/replay/not-a-number"), DashboardRoute::Home);
    /// ```
    pub fn parse(input: &str) -> Self {
        let normalized = normalize_route(input);
        let parts: Vec<&str> = normalized
            .trim_start_matches('/')
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect();

        match parts.as_slice() {
            [] => Self::Home,
            ["service", service] => Self::Service {
                service: service.to_string(),
            },
            ["service", service, "replay", interaction_id] => interaction_id
                .parse::<u64>()
                .map(|id| Self::Replay {
                    service: service.to_string(),
                    interaction_id: id,
                })
                .unwrap_or(Self::Home),
            _ => Self::Home,
        }
    }

    /// Convert the route into its canonical URL path string.
    ///
    /// Produces "/" for `Home`, "/service/{service}" for `Service`, and
    /// "/service/{service}/replay/{interaction_id}" for `Replay`.
    ///
    /// # Examples
    ///
    /// ```
    /// assert_eq!(DashboardRoute::Home.to_path(), "/");
    /// assert_eq!(DashboardRoute::Service { service: "api".into() }.to_path(), "/service/api");
    /// assert_eq!(DashboardRoute::Replay { service: "api".into(), interaction_id: 42 }.to_path(), "/service/api/replay/42");
    /// ```
    pub fn to_path(&self) -> String {
        match self {
            Self::Home => "/".to_string(),
            Self::Service { service } => format!("/service/{service}"),
            Self::Replay {
                service,
                interaction_id,
            } => format!("/service/{service}/replay/{interaction_id}"),
        }
    }

    /// Get the service name associated with the route, if any.
    ///
    /// # Returns
    ///
    /// `Some(&str)` containing the service name for `Service` and `Replay` variants, `None` for `Home`.
    ///
    /// # Examples
    ///
    /// ```
    /// let s = DashboardRoute::Service { service: "api".into() };
    /// assert_eq!(s.service(), Some("api"));
    ///
    /// let h = DashboardRoute::Home;
    /// assert_eq!(h.service(), None);
    /// ```
    pub fn service(&self) -> Option<&str> {
        match self {
            Self::Home => None,
            Self::Service { service } | Self::Replay { service, .. } => Some(service.as_str()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DashboardNavigationState {
    route: DashboardRoute,
    last_service: Option<String>,
}

impl DashboardNavigationState {
    /// Creates a new DashboardNavigationState initialized to the home route with no last selected service.
    ///
    /// # Examples
    ///
    /// ```
    /// let state = DashboardNavigationState::new();
    /// assert!(matches!(state.route(), DashboardRoute::Home));
    /// assert!(state.selected_service().is_none());
    /// ```
    pub fn new() -> Self {
        Self {
            route: DashboardRoute::Home,
            last_service: None,
        }
    }

    /// Creates a navigation state from a route, initializing the cached last service from the route if present.
    ///
    /// The resulting state has `route` set to the provided value and `last_service` set to the route's service name converted to an owned `String` when available.
    ///
    /// # Examples
    ///
    /// ```
    /// let route = DashboardRoute::Service { service: "api".into() };
    /// let state = DashboardNavigationState::from_route(route);
    /// assert_eq!(state.route().service(), Some("api"));
    /// assert_eq!(state.selected_service(), Some("api"));
    /// ```
    pub fn from_route(route: DashboardRoute) -> Self {
        let last_service = route.service().map(ToOwned::to_owned);
        Self {
            route,
            last_service,
        }
    }

    /// Create a `DashboardNavigationState` by parsing a path string.
    ///
    /// The input is parsed into a `DashboardRoute`, and the resulting route is used
    /// to initialize the navigation state.
    ///
    /// # Examples
    ///
    /// ```
    /// let state = DashboardNavigationState::from_path("/service/my-service");
    /// assert!(matches!(state.route(), DashboardRoute::Service { .. }));
    /// ```
    pub fn from_path(input: &str) -> Self {
        Self::from_route(DashboardRoute::parse(input))
    }

    /// Accesses the current dashboard route.
    ///
    /// # Examples
    ///
    /// ```
    /// let state = DashboardNavigationState::new();
    /// assert_eq!(state.route().to_path(), "/");
    /// ```
    ///
    /// # Returns
    ///
    /// A reference to the current `DashboardRoute`.
    pub fn route(&self) -> &DashboardRoute {
        &self.route
    }

    /// Serialize the navigation state into a canonical path string.
    ///
    /// The returned string represents the current route as a path, for example:
    /// "/", "/service/{service}", or "/service/{service}/replay/{interaction_id}".
    ///
    /// # Examples
    ///
    /// ```
    /// let mut state = DashboardNavigationState::new();
    /// assert_eq!(state.to_path(), "/");
    ///
    /// state.open_service("email");
    /// assert_eq!(state.to_path(), "/service/email");
    ///
    /// state.open_replay("email", 42);
    /// assert_eq!(state.to_path(), "/service/email/replay/42");
    /// ```
    pub fn to_path(&self) -> String {
        self.route.to_path()
    }

    /// Returns the currently selected service name, preferring the service from the active route and falling back to the last-known service.
    ///
    /// # Examples
    ///
    /// ```
    /// let mut state = DashboardNavigationState::new();
    /// assert!(state.selected_service().is_none());
    ///
    /// state.open_service("orders");
    /// assert_eq!(state.selected_service(), Some("orders"));
    ///
    /// state.go_home();
    /// // still remembers last service
    /// assert_eq!(state.selected_service(), Some("orders"));
    /// ```
    ///
    /// # Returns
    ///
    /// `Some(&str)` with the service name from the current route if present, otherwise the cached last service; `None` if neither is set.
    pub fn selected_service(&self) -> Option<&str> {
        self.route.service().or(self.last_service.as_deref())
    }

    /// Sets the navigation route to the dashboard home.
    ///
    /// # Examples
    ///
    /// ```
    /// let mut state = DashboardNavigationState::new();
    /// state.go_home();
    /// assert_eq!(state.to_path(), "/");
    /// ```
    pub fn go_home(&mut self) {
        self.route = DashboardRoute::Home;
    }

    /// Sets the current route to the Service view for the given service and updates the cached last service.
    ///
    /// # Examples
    ///
    /// ```
    /// let mut state = DashboardNavigationState::new();
    /// state.open_service("users");
    /// assert_eq!(state.route().to_path(), "/service/users");
    /// assert_eq!(state.selected_service(), Some("users"));
    /// ```
    pub fn open_service(&mut self, service: impl Into<String>) {
        let service = service.into();
        self.last_service = Some(service.clone());
        self.route = DashboardRoute::Service { service };
    }

    /// Navigate to the replay view for a specific service interaction.
    ///
    /// This sets the navigation route to the Replay variant for `service` with the given
    /// `interaction_id` and updates the remembered last service to `service`.
    ///
    /// # Examples
    ///
    /// ```
    /// let mut state = DashboardNavigationState::new();
    /// state.open_replay("orders", 42);
    /// assert_eq!(state.to_path(), "/service/orders/replay/42");
    /// assert_eq!(state.selected_service(), Some("orders"));
    /// ```
    pub fn open_replay(&mut self, service: impl Into<String>, interaction_id: u64) {
        let service = service.into();
        self.last_service = Some(service.clone());
        self.route = DashboardRoute::Replay {
            service,
            interaction_id,
        };
    }

    /// Navigate back to the most recently selected service, or go to the home route if none exists.
    ///
    /// # Examples
    ///
    /// ```
    /// let mut state = DashboardNavigationState::new();
    /// state.open_service("payments");
    /// state.open_replay("payments", 42);
    /// // return to the last selected service ("payments")
    /// state.back_to_service();
    /// assert_eq!(state.to_path(), "/service/payments");
    ///
    /// // if no service has been selected, go to home
    /// let mut state = DashboardNavigationState::new();
    /// state.back_to_service();
    /// assert_eq!(state.to_path(), "/");
    /// ```
    pub fn back_to_service(&mut self) {
        if let Some(service) = self.selected_service().map(ToOwned::to_owned) {
            self.route = DashboardRoute::Service { service };
        } else {
            self.route = DashboardRoute::Home;
        }
    }

    /// Updates the navigation state to the route parsed from the given path.
    ///
    /// If the parsed route contains a service name, that service becomes the new
    /// `last_service`. The state's current `route` is replaced by the parsed route.
    ///
    /// `input` may be a path or fragment (for example, "/service/foo" or "#/service/foo").
    ///
    /// # Examples
    ///
    /// ```
    /// let mut state = DashboardNavigationState::new();
    /// state.apply_path("/service/foo/replay/42");
    /// assert_eq!(state.selected_service(), Some("foo"));
    /// assert_eq!(state.to_path(), "/service/foo/replay/42");
    /// ```
    pub fn apply_path(&mut self, input: &str) {
        let parsed = DashboardRoute::parse(input);
        if let Some(service) = parsed.service() {
            self.last_service = Some(service.to_string());
        }
        self.route = parsed;
    }
}

impl Default for DashboardNavigationState {
    /// Creates a new DashboardNavigationState initialized to Home with no last_service.
    ///
    /// # Examples
    ///
    /// ```
    /// let state = DashboardNavigationState::default();
    /// assert_eq!(state.route(), &DashboardRoute::Home);
    /// assert!(state.selected_service().is_none());
    /// ```
    fn default() -> Self {
        Self::new()
    }
}

/// Normalize a route-like string into a canonical absolute path.

///

/// The function trims whitespace, removes a leading `#` fragment marker if present,

/// discards any query component (everything after the first `?`), and ensures the

/// result is a path that starts with `/`. If the resulting path is empty, `/` is returned.

///

/// # Examples

///

/// ```

/// assert_eq!(normalize_route("  /service/foo  "), "/service/foo");

/// assert_eq!(normalize_route("#/service/foo?tab=1"), "/service/foo");

/// assert_eq!(normalize_route("service/foo"), "/service/foo");

/// assert_eq!(normalize_route(""), "/");

/// ```
fn normalize_route(input: &str) -> String {
    let trimmed = input.trim();
    let without_hash = trimmed.strip_prefix('#').unwrap_or(trimmed);
    let path = without_hash.split('?').next().unwrap_or(without_hash);

    if path.is_empty() {
        "/".to_string()
    } else if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_parse_and_serialize_for_all_contexts() {
        let home = DashboardRoute::parse("/");
        assert_eq!(home, DashboardRoute::Home);
        assert_eq!(home.to_path(), "/");

        let service = DashboardRoute::parse("#/service/s3");
        assert_eq!(
            service,
            DashboardRoute::Service {
                service: "s3".to_string()
            }
        );
        assert_eq!(service.to_path(), "/service/s3");

        let replay = DashboardRoute::parse("/service/s3/replay/42?tab=history");
        assert_eq!(
            replay,
            DashboardRoute::Replay {
                service: "s3".to_string(),
                interaction_id: 42
            }
        );
        assert_eq!(replay.to_path(), "/service/s3/replay/42");
    }

    #[test]
    fn invalid_route_defaults_to_home() {
        assert_eq!(DashboardRoute::parse("/unknown/path"), DashboardRoute::Home);
        assert_eq!(
            DashboardRoute::parse("/service/s3/replay/not-a-number"),
            DashboardRoute::Home
        );
    }

    #[test]
    fn navigation_transitions_between_home_service_and_replay() {
        let mut state = DashboardNavigationState::new();
        assert_eq!(state.route(), &DashboardRoute::Home);
        assert_eq!(state.selected_service(), None);

        state.open_service("sqs");
        assert_eq!(
            state.route(),
            &DashboardRoute::Service {
                service: "sqs".to_string()
            }
        );
        assert_eq!(state.selected_service(), Some("sqs"));

        state.open_replay("sqs", 7);
        assert_eq!(
            state.route(),
            &DashboardRoute::Replay {
                service: "sqs".to_string(),
                interaction_id: 7
            }
        );
        assert_eq!(state.to_path(), "/service/sqs/replay/7");

        state.back_to_service();
        assert_eq!(
            state.route(),
            &DashboardRoute::Service {
                service: "sqs".to_string()
            }
        );

        state.go_home();
        assert_eq!(state.route(), &DashboardRoute::Home);
        assert_eq!(state.selected_service(), Some("sqs"));
    }

    #[test]
    fn apply_path_updates_route_and_context() {
        let mut state = DashboardNavigationState::new();
        state.apply_path("/service/s3");
        assert_eq!(state.selected_service(), Some("s3"));

        state.apply_path("/");
        assert_eq!(state.route(), &DashboardRoute::Home);
        assert_eq!(state.selected_service(), Some("s3"));
    }
}
