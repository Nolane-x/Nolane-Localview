# Linux L02 DEFUNCT Recreate Implementation Plan

1. Establish RED with `l02_defunct_recreate_contract.rs`: recreation must require a same-lineage previous binding already terminally invalidated by explicit DEFUNCT.
2. Add typed `AtspiReacquireError` and replace unrestricted `reacquire` with `reacquire_after_defunct`.
3. Re-run L01 and L02 package contracts; keep production authorization semantics unchanged.
4. Extend the real Linux seed/harness so a replacement accessible is created after the old accessible becomes queryable DEFUNCT, then prove old authority remains rejected while the replacement fresh binding can act.
5. Add/extend dedicated exact-head real-provider evidence for L02.
6. Require full CI plus retained Windows/macOS/L01 gates on one immutable head before merge.
