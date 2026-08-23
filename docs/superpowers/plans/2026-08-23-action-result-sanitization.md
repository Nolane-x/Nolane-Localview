# Action Result Sanitization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ensure completed `type_text` actions can never retain the typed value in LocalView's live result history, even if callers bypass the HTTP evidence-sanitization path.

**Architecture:** Move privacy enforcement into `localview-live-bridge`, the component that owns bounded action-result storage. Completion becomes action-aware: the bridge receives the claimed `BridgeAction`, derives a storage-safe `BridgeActionResult`, and only then appends to history. `localview-control` continues producing its richer redacted evidence payload independently, but no longer has any raw-storage escape hatch.

**Tech Stack:** Rust 1.98 workspace, Tokio, serde/serde_json, Axum control plane, GitHub Actions multi-OS CI.

**Spec:** `/mnt/data/LocalView_AI_Native_Localhost_Runtime_Product_Spec_v3_Expanded.md`

## Global Constraints

- Local-first; no cloud account dependency.
- Secret/user-entered text must not be retained in diagnostic/evidence history unless explicitly required.
- Keep the live bridge bounded and deterministic.
- Preserve current HTTP response contracts and action IDs/timestamps.
- No unsafe Rust.
- Full workspace `cargo check`, Clippy with `-D warnings`, tests, frontend build, and Tauri backend check remain required gates.

---

### Task 1: Make live result storage action-aware

**Files:**
- Modify: `crates/live-bridge/src/lib.rs`
- Modify: `crates/control/src/lib.rs`

**Interfaces:**
- Consumes: `BridgeAction`, `BridgeActionKind`, `BridgeActionResult`.
- Produces: `LiveBridge::complete_action(&self, action: &BridgeAction, result: BridgeActionResult)` where stored history is privacy-safe.

- [ ] **Step 1: Write the failing regression test**

Add a Tokio test that enqueues and claims a `BridgeActionKind::TypeText { text: "super-secret-value", clear_first: true }`, completes it with payload `{"value":"super-secret-value"}` and an error containing the same text, then reads `recent_results`. Assert serialized history does not contain `super-secret-value`, while `action_id`, `ok`, and `completed_at` remain intact.

- [ ] **Step 2: Run the test to verify RED**

Run: `cargo test -p localview-live-bridge completed_type_text_result_never_retains_typed_value -- --nocapture`
Expected: FAIL because the existing `complete_action(session_id, result)` stores the raw payload/error unchanged.

- [ ] **Step 3: Implement the minimal storage sanitizer**

Change completion to accept the claimed action. Before storage, if the action is `TypeText`, replace `result.payload` with `serde_json::Value::Null` and remove exact occurrences of the typed text from `result.error` (replace with `[REDACTED]`). Non-typing actions retain their existing result payloads.

- [ ] **Step 4: Update the control-plane call site**

After `claim_action`, call `state.live.complete_action(&action, result).await`. Keep evidence generation based on the already-sanitized evidence helper so the HTTP/evidence contract does not change.

- [ ] **Step 5: Verify GREEN and regressions**

Run the targeted test again, then `cargo check --workspace --exclude localview-desktop --all-targets`, `cargo clippy --workspace --exclude localview-desktop --all-targets -- -D warnings`, `cargo test --workspace --exclude localview-desktop`, frontend build, and Tauri backend check through the existing CI matrix.

- [ ] **Step 6: Commit**

Commit only the regression + privacy-boundary implementation as `fix: sanitize live action result storage`.
