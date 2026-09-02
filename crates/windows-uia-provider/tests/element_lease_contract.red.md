# Element lease RED checkpoint

This test-first slice intentionally requires a worker-owned exact UIA element lease API before production implementation exists.

Required invariants:
- exact current snapshot element binds successfully;
- publishing a newer snapshot invalidates the older snapshot lease immediately;
- exact `ProviderElementRef` equality is required, with no RuntimeId or semantic-locator fallback;
- leasing does not enable UIA write actions or input dispatch.
