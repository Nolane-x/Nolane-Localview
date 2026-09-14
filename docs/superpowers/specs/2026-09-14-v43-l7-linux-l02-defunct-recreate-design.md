# V4.3 Linux L02 DEFUNCT Recreate Design

## Scope

Close Linux seed L02: an AT-SPI accessible that has become explicitly `DEFUNCT` may be replaced only by minting fresh LocalView binding authority. Reuse of the same AT-SPI bus name and object path must never resurrect the old binding.

## Authority boundary

- L01 remains authoritative for observing typed `State::Defunct` and terminally invalidating the old binding.
- L02 recreation is a separate transition: the caller must present the previous binding as evidence that it is already `InvalidDefunct`.
- The previous binding must belong to the same provider incarnation and target incarnation as the provider performing recreation.
- Recreation before explicit DEFUNCT is rejected. Observation transport failure is not equivalent to DEFUNCT and must not authorize recreation.
- A successful recreation mints a new binding revision and a new acquisition cut. The old binding remains terminally invalid forever.

## Production API

Replace unrestricted `LinuxAtspiProvider::reacquire(...)` with:

`reacquire_after_defunct(previous_binding, new_endpoint, new_acquisition_cut_ref)`

The method returns a typed `AtspiReacquireError` for:

- previous binding not explicitly DEFUNCT;
- provider-incarnation mismatch;
- target-incarnation mismatch.

No object fingerprint or heuristic identity inference is added in L02; the trusted transition is explicit DEFUNCT -> fresh binding authority.

## Verification

1. TDD contract starts RED because the typed recreation API does not exist.
2. GREEN implementation must preserve all L01 behavior.
3. Real Ubuntu GTK/ATK/AT-SPI evidence will exercise old live object -> typed DEFUNCT -> replacement object -> fresh binding, proving the old binding cannot authorize while the replacement can.
4. Exact-head L01, L02, full CI, retained Windows, and retained macOS gates must converge before merge.
