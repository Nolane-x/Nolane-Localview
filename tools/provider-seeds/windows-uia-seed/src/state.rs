use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroundTruth {
    pub seed_run_id: Uuid,
    pub process_incarnation: Uuid,
    pub window_handle: u64,
    pub control_handle: u64,
    pub control_incarnation: Uuid,
    pub logical_name: String,
    pub unsupported_invoke_control_handle: u64,
    pub unsupported_invoke_side_effect_count: u64,
    pub recreation_generation: u64,
    pub logical_sequence: u64,
    pub terminal: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeedState {
    ground_truth: GroundTruth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeedStateError {
    Terminal,
    EmptyNameBurst,
}

impl fmt::Display for SeedStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Terminal => "seed process is terminal",
            Self::EmptyNameBurst => "name-change burst must contain at least one mutation",
        })
    }
}

impl std::error::Error for SeedStateError {}

impl SeedState {
    pub fn new(
        seed_run_id: Uuid,
        process_incarnation: Uuid,
        window_handle: u64,
        control_handle: u64,
        control_incarnation: Uuid,
        logical_name: String,
        unsupported_invoke_control_handle: u64,
    ) -> Self {
        Self {
            ground_truth: GroundTruth {
                seed_run_id,
                process_incarnation,
                window_handle,
                control_handle,
                control_incarnation,
                logical_name,
                unsupported_invoke_control_handle,
                unsupported_invoke_side_effect_count: 0,
                recreation_generation: 1,
                logical_sequence: 1,
                terminal: false,
            },
        }
    }

    pub fn ground_truth(&self) -> GroundTruth {
        self.ground_truth.clone()
    }

    pub fn burst_name_changes(
        &mut self,
        names: &[String],
    ) -> Result<GroundTruth, SeedStateError> {
        self.ensure_live()?;
        if names.is_empty() {
            return Err(SeedStateError::EmptyNameBurst);
        }

        for name in names {
            self.ground_truth.logical_name = name.clone();
            self.ground_truth.logical_sequence = self
                .ground_truth
                .logical_sequence
                .checked_add(1)
                .expect("seed logical sequence must remain bounded in tests");
        }
        Ok(self.ground_truth())
    }

    pub fn record_recreated_control(
        &mut self,
        control_handle: u64,
        control_incarnation: Uuid,
    ) -> Result<GroundTruth, SeedStateError> {
        self.ensure_live()?;
        self.ground_truth.control_handle = control_handle;
        self.ground_truth.control_incarnation = control_incarnation;
        self.ground_truth.recreation_generation = self
            .ground_truth
            .recreation_generation
            .checked_add(1)
            .expect("seed recreation generation must remain bounded in tests");
        self.ground_truth.logical_sequence = self
            .ground_truth
            .logical_sequence
            .checked_add(1)
            .expect("seed logical sequence must remain bounded in tests");
        Ok(self.ground_truth())
    }

    pub fn shutdown(&mut self) -> Result<GroundTruth, SeedStateError> {
        self.ensure_live()?;
        self.ground_truth.terminal = true;
        self.ground_truth.logical_sequence = self
            .ground_truth
            .logical_sequence
            .checked_add(1)
            .expect("seed logical sequence must remain bounded in tests");
        Ok(self.ground_truth())
    }

    fn ensure_live(&self) -> Result<(), SeedStateError> {
        if self.ground_truth.terminal {
            Err(SeedStateError::Terminal)
        } else {
            Ok(())
        }
    }
}
