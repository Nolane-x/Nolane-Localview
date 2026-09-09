#!/usr/bin/env bash
set -euo pipefail

python scripts/v43-task8-wire-control-state.py
cargo fmt --all
cargo test -p localview-control --lib 'windows_consequential::' -- --nocapture
cargo check -p localview-control
cargo clippy -p localview-control --all-targets -- -D warnings
git diff --check

rm scripts/v43-task8-wire-control-state.py
rm scripts/v43-task8-run-control-state.sh
rm .github/workflows/v43-task8-runner-probe.yml

git config user.name 'github-actions[bot]'
git config user.email '41898282+github-actions[bot]@users.noreply.github.com'
git add -A -- \
  crates/control/src/windows_consequential.rs \
  scripts/v43-task8-wire-control-state.py \
  scripts/v43-task8-run-control-state.sh \
  .github/workflows/v43-task8-runner-probe.yml
git diff --cached --check
git commit -m 'feat(v43): bind SetValue state to control lifetime'
git push origin HEAD:feat/v43-windows-set-value-payload-authority
