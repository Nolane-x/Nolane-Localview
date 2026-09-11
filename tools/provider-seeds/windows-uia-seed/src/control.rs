use serde::{Deserialize, Serialize};

use crate::GroundTruth;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "command")]
pub enum SeedCommand {
    GetGroundTruth,
    BurstNameChanges { names: Vec<String> },
    RecreateControl,
    PresentUnsupportedInvokeControl,
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "response")]
pub enum SeedResponse {
    Ready { ground_truth: GroundTruth },
    GroundTruth { ground_truth: GroundTruth },
    Applied { ground_truth: GroundTruth },
    Error { code: String, message: String },
}

impl SeedResponse {
    pub fn ground_truth(ground_truth: GroundTruth) -> Self {
        Self::GroundTruth { ground_truth }
    }

    pub fn applied(ground_truth: GroundTruth) -> Self {
        Self::Applied { ground_truth }
    }

    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Error {
            code: code.into(),
            message: message.into(),
        }
    }
}
