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
    pub fn new() -> Self {
        Self {
            route: DashboardRoute::Home,
            last_service: None,
        }
    }

    pub fn from_route(route: DashboardRoute) -> Self {
        let last_service = route.service().map(ToOwned::to_owned);
        Self {
            route,
            last_service,
        }
    }

    pub fn from_path(input: &str) -> Self {
        Self::from_route(DashboardRoute::parse(input))
    }

    pub fn route(&self) -> &DashboardRoute {
        &self.route
    }

    pub fn to_path(&self) -> String {
        self.route.to_path()
    }

    pub fn selected_service(&self) -> Option<&str> {
        self.route.service().or(self.last_service.as_deref())
    }

    pub fn go_home(&mut self) {
        self.route = DashboardRoute::Home;
    }

    pub fn open_service(&mut self, service: impl Into<String>) {
        let service = service.into();
        self.last_service = Some(service.clone());
        self.route = DashboardRoute::Service { service };
    }

    pub fn open_replay(&mut self, service: impl Into<String>, interaction_id: u64) {
        let service = service.into();
        self.last_service = Some(service.clone());
        self.route = DashboardRoute::Replay {
            service,
            interaction_id,
        };
    }

    pub fn back_to_service(&mut self) {
        if let Some(service) = self.selected_service().map(ToOwned::to_owned) {
            self.route = DashboardRoute::Service { service };
        } else {
            self.route = DashboardRoute::Home;
        }
    }

    pub fn apply_path(&mut self, input: &str) {
        let parsed = DashboardRoute::parse(input);
        if let Some(service) = parsed.service() {
            self.last_service = Some(service.to_string());
        }
        self.route = parsed;
    }
}

impl Default for DashboardNavigationState {
    fn default() -> Self {
        Self::new()
    }
}

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
