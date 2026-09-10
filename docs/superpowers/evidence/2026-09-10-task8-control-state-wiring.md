# Task 8 Control-State Wiring Evidence

Exact verified implementation commit: `2b0719d7f1b40f375c08f85ec8fa1b6ce4f8bab6`

The dedicated Task 8 verifier applied the control-lifetime wiring, then passed:

- `cargo test -p localview-control --lib 'windows_consequential::' -- --nocapture` — 9 passed, 0 failed.
- `cargo check -p localview-control`.
- `cargo clippy -p localview-control --all-targets -- -D warnings`.
- `git diff --check` and staged diff checks.

The implementation binds `WindowsSetValuePayloadAuthority` to `WindowsConsequentialControlHandle`, constructs a fresh process-local authority on control configuration, and releases pending SetValue payload authority with session teardown. Temporary runner files were removed by the verified implementation commit.

## Missing-operation cleanup closure

A later fail-closed audit added `windows_consequential_set_value_missing_operation_cleanup.rs` as a test-only RED at `2186c23fd6ef17f832ce21e54a5d6375b9e556cd`. CI #1557 failed deterministically in the Rust-core workspace tests on Ubuntu, macOS, and Windows because the `Ok(None)` branch for a missing durable canonical-operation binding consumed the generic confirmation but did not consume an exact staged SetValue payload capability.

The production closure removes that orphaned process-local plaintext authority under the existing `plan_gate`: after the exact generic confirmation is consumed, a matching staged SetValue capability is also consumed through `consume_verified` before the fail-closed conflict response is returned. This does not mint PREPARED or dispatch authority and does not change the normal SetValue execution path.

The one-shot scoped verifier run `34459471765` then passed all of its gates before creating production commit `0640a75ab00c1091ce78384452370ab220045138`:

- scoped `rustfmt` changed only `crates/control/src/windows_consequential.rs`;
- `cargo test -p localview-control --test windows_consequential_set_value_missing_operation_cleanup -- --nocapture`;
- `cargo check -p localview-control`;
- `cargo clippy -p localview-control --all-targets --no-deps -- -D warnings`;
- removal of both one-shot formatter workflows in the same production commit.

The bot-authored production commit itself received GitHub `action_required` workflow classifications without jobs, so this evidence update intentionally creates an ordinary owner-authored head for fresh exact-head CI and Windows UIA verification. No SetValue plaintext payload is present in this evidence file.