# LocalView production release gates

LocalView desktop is built as a Tauri application with the `localview-daemon` embedded as a target-specific sidecar.

## Runtime packaging invariant

A desktop build is not considered usable unless:

1. the matching `localview-daemon-$TARGET_TRIPLE` binary is prepared;
2. Tauri bundles that binary through `bundle.externalBin`;
3. desktop startup accepts an already-running same-version daemon or starts the bundled daemon;
4. `/health` reports `ready` with the same LocalView version;
5. dashboard WebViews receive no generic shell permission.

`apps/desktop/scripts/prepare-sidecar.mjs` is called automatically by Tauri's development and production build hooks.

## Local development

```bash
cd apps/desktop
npm install
npm run tauri dev
```

The development hook builds a debug daemon sidecar before Vite starts.

## Local release candidate

```bash
cd apps/desktop
npm install
npm run tauri build
```

The production hook builds the daemon in release mode, copies it to the target-triple sidecar name expected by Tauri, then creates platform bundles.

## CI release candidate

Run the **Desktop release candidate** workflow manually. It builds Windows, macOS, and Linux bundles and uploads `target/release/bundle/**` only when the bundle exists.

These CI artifacts are **release candidates**, not signed public releases.

## Public-release gates still required

Before publishing installers to end users, all of the following must be configured and proven on exact-head CI:

- Windows code signing identity and signed installer verification;
- macOS Developer ID signing and notarization;
- a user-triggered trusted update-check channel may ship before signing credentials exist, but automatic update download/install requires production update signing keys and signature-verification authority;
- repeat installation/launch smoke on the final signed/notarized public artifacts; R14 already closes the unsigned release-candidate software gate on fresh hosted runners for Linux, macOS and Windows;
- upgrade and rollback tests across at least the previous supported version; for the initial supported release, where no prior supported tag exists, CI must instead prove the declared initial-release policy and rollback-readable persisted-state compatibility;
- provenance/SBOM publication if the release policy requires them.

Do not label an unsigned CI artifact as a production public release.


## Initial supported release policy

`release-policy.json` is the executable authority for whether a previous supported release exists.

For the current 0.2.0 first supported release:

- no prior supported Git tag exists, so inventing an installer predecessor is forbidden;
- Trusted Verify primary recovery metadata remains schema-v1 readable;
- Wave 9-only recovery context lives in a companion file ignored by the prior reader contract;
- the companion commits before the rollback-readable primary metadata commit-point;
- CI verifies the policy, exact product version, absence/presence of supported release tags and the rollback-state contract.

After the first supported release is tagged, `initial_supported_release` must become false. Every subsequent release must declare `previous_supported_version` and enable both installer upgrade and installer rollback evidence; the policy verifier fails closed otherwise.


## V1 update boundary

R16 defines a check-only update path. The manifest URL is injected at compile time through `LOCALVIEW_UPDATE_MANIFEST_URL`; if absent, Settings reports that update checking is not configured and performs no network request.

When configured, the check path:

- accepts HTTPS on the default port only;
- refuses redirects, credentials, query strings and fragments;
- bounds the manifest body to 64 KiB;
- requires the fixed `localview-update-manifest-v1` / `stable` schema;
- requires exactly one current OS/architecture artifact;
- requires a full candidate Git object id and canonical SHA-256 metadata;
- requires artifact metadata to remain on the same pinned origin as the manifest;
- never downloads or installs the artifact.

A detached signature field in the manifest is informational only in R16. It never sets `installAuthorized`. Signed automatic update installation remains externally blocked until a production signing key and concrete signature-verification authority are configured and proven.
