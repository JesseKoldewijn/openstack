/// States a service can be in during its lifecycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceState {
    /// Service is registered but not yet started.
    Available,
    /// Service is in the process of starting.
    Starting,
    /// Service is running and accepting requests.
    Running,
    /// Service is in the process of stopping.
    Stopping,
    /// Service has been stopped.
    Stopped,
    /// Service encountered an error.
    Error(String),
}

impl ServiceState {
    #[allow(dead_code)]
    fn to_u8(&self) -> u8 {
        match self {
            ServiceState::Available => 0,
            ServiceState::Starting => 1,
            ServiceState::Running => 2,
            ServiceState::Stopping => 3,
            ServiceState::Stopped => 4,
            ServiceState::Error(_) => 5,
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            ServiceState::Available => "available",
            ServiceState::Starting => "starting",
            ServiceState::Running => "running",
            ServiceState::Stopping => "stopping",
            ServiceState::Stopped => "stopped",
            ServiceState::Error(_) => "error",
        }
    }
}
