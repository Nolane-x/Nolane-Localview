# Task 8 Control-State Wiring Evidence

Exact verified implementation commit: `2b0719d7f1b40f375c08f85ec8fa1b6ce4f8bab6`

The dedicated Task 8 verifier applied the control-lifetime wiring, then passed:

- `cargo test -p localview-control --lib 'windows_consequential::' -- --nocapture` — 9 passed, 0 failed.
- `cargo check -p localview-control`.
- `cargo clippy -p localview-control --all-targets -- -D warnings`.
- `git diff --check` and staged diff checks.

The implementation binds `WindowsSetValuePayloadAuthority` to `WindowsConsequentialControlHandle`, constructs a fresh process-local authority on control configuration, and releases pending SetValue payload authority with session teardown. Temporary runner files were removed by the verified implementation commit.

This evidence file intentionally contains no SetValue plaintext payload and exists to trigger ordinary owner-authored exact-head PR verification after the implementation commit was created by `github-actions[bot]`, whose pull-request workflows were classified by GitHub as `action_required` without jobs.
