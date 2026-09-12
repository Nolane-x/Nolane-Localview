# V4.3 W05 worker poison GREEN evidence

This note records the bounded production worker-health RED→GREEN chain for PR #113. It does not claim the W05 real-provider hang seed, six-seed L7 campaign, PR #113, or V4.3 complete.

- RED `4cbeac87f7ab2731952e979559bd2a1a00795b4f`: Windows `cargo check --all-targets` reached the new W05 contract and failed because `worker_health.rs` did not exist.
- Stronger RED `6b8ab1150e0bfb80affa37a3f762fead48201217`: the health state existed, but the contract failed specifically because `WorkerReceiveError` and timeout-driven `WorkerHealth::recv_timeout` were absent.
- Helper GREEN `2219d00dc40c401011b0b000b01b621d57ec4625`: timeout receipt now poisons the shared health state and a poisoned health check is typed.
- Production one-shot candidate `47c5688b55bd069f6e47263dcaeb123fa14d98aa`: deterministic patching added `WorkerPoisoned`, primary MTA worker health, a pre-send health fence on all nine command paths, and timeout-driven poison mapping. On the generated production tree, `worker_timeout_poison_contract` and `cargo check -p localview-windows-uia-provider --all-targets` both passed before the implementation was committed.
- Production implementation commit `ecd6f39d52e51f4d0adbbba28b15827fa1c6dcc7`: `feat(v43): poison Windows UIA worker after command timeout`. The temporary patcher/workflow were removed by that commit.

The next authority boundary remains W05 real-provider behavior: a deterministic hostile UIA provider call must time out boundedly, the old worker must fail fast thereafter, reacquisition must create a fresh provider incarnation, and stale authority from the poisoned incarnation must be rejected.
