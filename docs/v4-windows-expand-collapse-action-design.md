# Windows UIA Expand/Collapse consequential execution

## Scope

This slice extends the verified Windows UIA consequential-action path from Invoke, SelectionItem and Toggle to payload-free ExpandCollapse semantics.

## Canonical authority

`Expand` and `Collapse` are distinct canonical operations. They both require the UIA `ExpandCollapse` capability, but sharing a UIA pattern must never collapse their durable operation identity into one ambiguous generic action.

The client may choose only the server-owned planning endpoint. It may not supply the native UIA pattern, provider method, risk class, idempotency class, decision principal, or any execution verb in JSON.

## Provider execution

The final provider-side operation must preserve the exact canonical operation through the sealed/durable execution path so the worker can select exactly one COM method:

- `Expand` -> `IUIAutomationExpandCollapsePattern::Expand()`
- `Collapse` -> `IUIAutomationExpandCollapsePattern::Collapse()`

Pattern capability alone is evidence, not authority. A request that proves only `ExpandCollapse=Supported` is insufficient to choose between the two methods.

## World-state evidence

Fresh semantic snapshots publish `windows_uia.expand_collapse.state` only when `ExpandCollapse` is supported and the current state is read successfully:

- `0` -> `collapsed`
- `1` -> `expanded`
- `2` -> `partially_expanded`
- `3` -> `leaf`

If the pattern is supported but state cannot be read, converted, or falls outside that domain, the provider records `uia_property_expand_collapse_state_unavailable`, omits the state attribute, and the snapshot is incomplete. Consequential postcondition verification therefore fails closed.

## Risk and retry policy

Both operations retain the conservative `S4 destructive_or_irreversible` and `irreversible` floor in this slice. No blind retry is authorized. A dispatch receipt proves only that the exact provider method was attempted; durable commit still requires an independent fresh post-dispatch snapshot satisfying the typed state postcondition.

## End-to-end acceptance

A real Win32 ComboBox smoke must execute this sequence through HTTP and the full durable coordinator:

1. start collapsed and observe `ExpandCollapse=Supported`;
2. plan server-owned `Expand` against a fresh cut;
3. explicitly confirm;
4. execute `Expand()` on the exact retained element;
5. capture a fresh snapshot and verify `state=expanded` before durable commit;
6. plan server-owned `Collapse` from the fresh expanded snapshot;
7. explicitly confirm;
8. execute `Collapse()` on the exact retained element;
9. capture another fresh snapshot and verify `state=collapsed` before durable commit.

The legacy V1-V3 public action queue must remain empty throughout.