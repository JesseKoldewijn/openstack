use serde::{Deserialize, Serialize};

/// STS is stateless for our purposes — no persistent credentials needed.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct StsStore {}

impl StsStore {
    pub fn new() -> Self {
        Self::default()
    }
}
