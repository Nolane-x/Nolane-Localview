# V4.3 Linux L04 — Missing Toolkit Event + Direct Reconciliation

Base: `main@d53031bb68612431c6be7adc01d00b6c6aed01f7`

Scope is bounded to Linux L04. No new reconciliation subsystem is introduced.

Authority contract:
- toolkit/event silence never proves no state change;
- Linux AT-SPI event reliability remains explicitly incomplete for toolkit-sensitive dimensions;
- event-derived cached observations cannot mint consequential action freshness when direct reconciliation is required;
- direct AT-SPI observation creates a fresh immutable observation revision/cut;
- old cached observations remain immutable;
- current snapshot completeness may be restored by reconciliation without rewriting event continuity into a stronger claim;
- observations are bound to provider/target/binding identity and cannot cross-bind silently;
- L01 DEFUNCT, L02 recreation freshness, and L03 pointer occlusion semantics remain retained.

TDD lineage:
1. RED contract introduces the wished-for reliability/reconciliation API and must fail before production code exists.
2. Minimal production implementation closes the semantic contract.
3. Real GTK3/ATK/AT-SPI seed suppresses the relevant toolkit event while direct AT-SPI state changes, proving direct reconciliation catches the missed event without seed side-channel authority.
4. Exact-head retained Linux/macOS/Windows campaigns + full CI must converge GREEN before merge.
