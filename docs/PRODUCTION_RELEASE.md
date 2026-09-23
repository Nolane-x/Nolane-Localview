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
- update signing keys plus a trusted update manifest/channel;
- installation/launch smoke on clean machines;
- upgrade and rollback tests across at least the previous supported version;
- provenance/SBOM publication if the release policy requires them.

Do not label an unsigned CI artifact as a production public release.
