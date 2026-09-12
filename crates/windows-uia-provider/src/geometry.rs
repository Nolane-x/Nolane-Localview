use localview_protocol::{ProviderElementRef, ProviderIncarnationRef, TargetIncarnationRef};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsUiaCoordinateSpace {
    PhysicalScreenPixels,
}

impl WindowsUiaCoordinateSpace {
    pub const fn as_wire_value(self) -> &'static str {
        match self {
            Self::PhysicalScreenPixels => "physical_screen_pixels",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowsUiaPhysicalRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl WindowsUiaPhysicalRect {
    pub fn new(
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    ) -> Result<Self, WindowsUiaGeometryContractError> {
        if right < left || bottom < top {
            return Err(WindowsUiaGeometryContractError::InvertedRectangle);
        }
        Ok(Self {
            left,
            top,
            right,
            bottom,
        })
    }

    pub fn width(self) -> i64 {
        i64::from(self.right) - i64::from(self.left)
    }

    pub fn height(self) -> i64 {
        i64::from(self.bottom) - i64::from(self.top)
    }
}

#[derive(Debug, Clone, Copy, Error, PartialEq, Eq)]
pub enum WindowsUiaGeometryContractError {
    #[error("Windows UI Automation geometry request requires a snapshot cut")]
    MissingSnapshotCut,
    #[error("Windows UI Automation geometry element does not belong to the requested snapshot cut")]
    ElementSnapshotCutMismatch,
    #[error("Windows UI Automation physical rectangle is inverted")]
    InvertedRectangle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsUiaGeometryRequest {
    snapshot_cut_ref: String,
    element_ref: ProviderElementRef,
}

impl WindowsUiaGeometryRequest {
    pub fn new(
        snapshot_cut_ref: impl Into<String>,
        element_ref: ProviderElementRef,
    ) -> Result<Self, WindowsUiaGeometryContractError> {
        let snapshot_cut_ref = snapshot_cut_ref.into();
        if snapshot_cut_ref.trim().is_empty() {
            return Err(WindowsUiaGeometryContractError::MissingSnapshotCut);
        }
        if element_ref.acquisition_cut_ref != snapshot_cut_ref {
            return Err(WindowsUiaGeometryContractError::ElementSnapshotCutMismatch);
        }
        Ok(Self {
            snapshot_cut_ref,
            element_ref,
        })
    }

    pub fn snapshot_cut_ref(&self) -> &str {
        &self.snapshot_cut_ref
    }

    pub fn element_ref(&self) -> &ProviderElementRef {
        &self.element_ref
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsUiaGeometryReceipt {
    pub snapshot_cut_ref: String,
    pub provider_incarnation_ref: ProviderIncarnationRef,
    pub target_incarnation_ref: TargetIncarnationRef,
    pub element_ref: ProviderElementRef,
    pub coordinate_space: WindowsUiaCoordinateSpace,
    pub bounding_rect: WindowsUiaPhysicalRect,
    pub target_window_dpi: u32,
}
