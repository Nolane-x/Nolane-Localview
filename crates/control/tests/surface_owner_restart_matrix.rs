#![forbid(unsafe_code)]

use std::{fs, path::PathBuf};

fn contract_source(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join(name);
    fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("required D2 restart contract {} is unavailable: {error}", path.display())
    })
}

fn require_all(source: &str, scenario: &str, markers: &[&str]) {
    for marker in markers {
        assert!(
            source.contains(marker),
            "permanent D2 restart matrix lost {scenario} marker {marker}"
        );
    }
}

#[test]
fn permanent_d2_restart_matrix_remains_backed_by_behavioral_contracts() {
    let restart = contract_source("surface_owner_restart.rs");
    require_all(
        &restart,
        "daemon restart + desktop survives",
        &[
            "exact_debt_reattaches_same_durable_session_with_fresh_boot_authority",
            "assert_ne!(second_owner.boot_epoch, first_owner.boot_epoch)",
            "assert_ne!(second_owner.owner_lease_id, first_owner.owner_lease_id)",
            "exact surviving owner debt must create fresh current-boot live authority",
        ],
    );
    require_all(
        &restart,
        "both restart",
        &["replacement_owner", "surface_recovery_debt_missing"],
    );

    let liveness = contract_source("surface_owner_liveness.rs");
    require_all(
        &liveness,
        "desktop restart + daemon survives",
        &[
            "expired_owner_reaper_drops_pending_and_live_governor_state_without_transferring_debt",
            "surface_owner_not_registered",
            "a replacement owner must never adopt the expired owner's recovery debt",
            "expired owner resources must return NativeSurface governor accounting to baseline",
        ],
    );

    let fence = contract_source("surface_owner_fence.rs");
    require_all(
        &fence,
        "same SessionId + same label + incarnation=1 + different owner",
        &[
            "live_surface_mutations_are_fenced_by_exact_owner_instance_and_current_lease",
            "surface_owner_fence_mismatch",
        ],
    );
    require_all(
        &fence,
        "stale boot epoch",
        &[
            "current_boot_registration_fences_surface_reservations",
            "surface_owner_boot_epoch_mismatch",
        ],
    );
    require_all(
        &fence,
        "stale owner lease",
        &[
            "surface_owner_lease_mismatch",
            "lease rotation must preserve the same process owner identity while revoking the old capability",
        ],
    );

    let journal = contract_source("surface_recovery_journal.rs");
    require_all(
        &journal,
        "corrupt recovery journal",
        &[
            "corrupt_recovery_journal_fails_closed",
            "corruption must never be laundered into an empty recovery inventory",
        ],
    );
}
