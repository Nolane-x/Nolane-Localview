from pathlib import Path

path = Path("crates/windows-observe-runtime/src/runtime_manager.rs")
text = path.read_text()
anchor = "    pub async fn read_semantic(\n"
method = r'''    /// Return the immutable semantic revision currently bound to one attached session.
    ///
    /// This is observation evidence only. The returned revision does not reserve
    /// an element, authorize a write, or prevent a later reconciliation. Any
    /// consequential action must still pass preflight, exact lease binding,
    /// dispatch-time context revalidation, durable PREPARED, and one-shot dispatch.
    pub async fn current_semantic_snapshot(
        &self,
        session_id: SessionId,
    ) -> Option<Arc<NativeSemanticSnapshotRevision>> {
        let _gate = self.operation_gate.lock().await;
        self.active
            .lock()
            .await
            .get(&session_id)
            .map(|observation| observation.current_snapshot.clone())
    }

'''
if method.strip() in text:
    raise SystemExit("current_semantic_snapshot already present")
if text.count(anchor) != 1:
    raise SystemExit(f"expected exactly one read_semantic anchor, found {text.count(anchor)}")
path.write_text(text.replace(anchor, method + anchor, 1))
