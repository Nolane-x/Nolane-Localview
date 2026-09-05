use localview_live_bridge::{
    ConsequentialRecoveryDebtDisposition, ConsequentialRecoveryState,
};

#[test]
fn durable_recovery_states_have_fail_closed_debt_dispositions() {
    use ConsequentialRecoveryDebtDisposition::{
        CommitOnly, HistoricalTerminal, NoDispatchProven, ObservationRequired,
        ReconciliationRequired,
    };
    use ConsequentialRecoveryState::{
        Admitted, AuthorizedNotDispatched, Committed, Compensated, CompensationFailed,
        DispatchPrepared, KnownNotDispatched, OutcomeObservedUnverified, PossiblyDispatched,
        VerifiedUncommitted,
    };

    let cases = [
        (Admitted, NoDispatchProven),
        (AuthorizedNotDispatched, NoDispatchProven),
        // PREPARED is intentionally uncertain after restart even without a
        // dispatch receipt: recovery observes current state and never retries.
        (DispatchPrepared, ObservationRequired),
        (KnownNotDispatched, NoDispatchProven),
        (PossiblyDispatched, ObservationRequired),
        (OutcomeObservedUnverified, ObservationRequired),
        (VerifiedUncommitted, CommitOnly),
        (Compensated, HistoricalTerminal),
        // Failed compensation is not historical success and still needs an
        // explicit reconciliation/repair authority.
        (CompensationFailed, ReconciliationRequired),
        (Committed, HistoricalTerminal),
    ];

    for (state, expected) in cases {
        assert_eq!(state.recovery_debt_disposition(), expected, "state={state:?}");
    }
}
