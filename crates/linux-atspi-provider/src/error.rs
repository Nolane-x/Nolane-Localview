#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AtspiActionEligibilityError {
    #[error("AT-SPI accessible is DEFUNCT")]
    Defunct,
    #[error("AT-SPI binding was already invalidated as DEFUNCT")]
    AlreadyInvalidDefunct,
    #[error("AT-SPI state observation is unavailable")]
    ObservationUnavailable,
    #[error("AT-SPI provider incarnation does not match the binding")]
    ProviderIncarnationMismatch,
    #[error("AT-SPI target incarnation does not match the binding")]
    TargetIncarnationMismatch,
}
