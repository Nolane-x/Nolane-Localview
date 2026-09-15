use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ACCESSIBILITY_BUS_INCARNATION: AtomicU64 = AtomicU64::new(1);

/// Monotonic identity for one live AT-SPI accessibility-bus incarnation.
///
/// This identity is intentionally independent from both the LocalView provider
/// incarnation and the target application incarnation. Restarting only the
/// accessibility bus must therefore invalidate all authority minted from the
/// previous bus without pretending that either process restarted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AtspiAccessibilityBusIncarnationRef(u64);

impl AtspiAccessibilityBusIncarnationRef {
    pub(crate) fn fresh() -> Self {
        Self(NEXT_ACCESSIBILITY_BUS_INCARNATION.fetch_add(1, Ordering::Relaxed))
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtspiAccessibilityBusLifecycle {
    Connected,
    Disconnected,
}
