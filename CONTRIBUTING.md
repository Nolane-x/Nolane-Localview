# Contributing to LocalView

LocalView is intentionally modular. Put reusable runtime behavior in a focused crate, expose a small typed interface, and keep CLI/MCP/Tauri as thin consumers.

## Quality gate

```bash
cargo fmt --all -- --check
cargo check --workspace --exclude localview-desktop --all-targets
cargo clippy --workspace --exclude localview-desktop --all-targets -- -D warnings
cargo test --workspace --exclude localview-desktop
```

For desktop work:

```bash
cd apps/desktop
npm install
npm run build
cd ../..
cargo check -p localview-desktop
```

New deterministic behavior should have a regression test. Heuristics must expose evidence/confidence and must not be mislabeled as deterministic truth. Heavy browser dependencies require an explicit engine-escalation justification.
