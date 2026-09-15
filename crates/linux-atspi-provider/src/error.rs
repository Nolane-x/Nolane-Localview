#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AtspiProviderConnectionError {
    #[error("AT-SPI accessibility bus is unavailable")]
    AccessibilityBusUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AtspiBindError {
    #[error("AT-SPI endpoint was already bound and requires an explicit recreation transition")]
    EndpointAlreadyBound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AtspiReacquireError {
    #[error("previous AT-SPI binding is not terminally invalidated as DEFUNCT")]
    PreviousBindingNotDefunct,
    #[error("previous AT-SPI binding authority was already superseded")]
    PreviousBindingSuperseded,
    #[error("AT-SPI replacement endpoint already has binding history")]
    ReplacementEndpointAlreadyBound,
    #[error("AT-SPI provider incarnation does not match the previous binding")]
    ProviderIncarnationMismatch,
    #[error("AT-SPI target incarnation does not match the previous binding")]
    TargetIncarnationMismatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AtspiActionEligibilityError {
    #[error("AT-SPI accessible is DEFUNCT")]
    Defunct,
    #[error("AT-SPI binding was already invalidated as DEFUNCT")]
    AlreadyInvalidDefunct,
    #[error("AT-SPI state observation is unavailable")]
    ObservationUnavailable,
    #[error("AT-SPI event-derived state requires direct reconciliation before action authority")]
    ReconciliationRequired,
    #[error("AT-SPI state observation does not belong to the current binding")]
    ObservationBindingMismatch,
    #[error("AT-SPI provider incarnation does not match the binding")]
    ProviderIncarnationMismatch,
    #[error("AT-SPI target incarnation does not match the binding")]
    TargetIncarnationMismatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AtspiPointerEligibilityError {
    #[error("AT-SPI semantic eligibility failed: {0}")]
    Semantic(AtspiActionEligibilityError),
    #[error("AT-SPI target is not VISIBLE")]
    NotVisible,
    #[error("AT-SPI target is not SHOWING")]
    NotShowing,
    #[error("AT-SPI pointer hit-test is unavailable")]
    HitTestUnavailable,
    #[error("AT-SPI pointer hit-test resolves an accessible other than the intended target")]
    Occluded,
}
