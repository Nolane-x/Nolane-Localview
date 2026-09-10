#!/usr/bin/env bash
set -euo pipefail

python scripts/v43-task8-wire-control-state.py
python - <<'PY'
from pathlib import Path

path = Path('crates/control/src/windows_consequential/set_value_http.rs')
source = path.read_text()
old = '    payload: ProcessLocalSetValuePayload,\n'
new = '''    #[expect(\n        dead_code,\n        reason = "Task 8 payload remains process-local but is not read until Stage 3 exact-confirmation dispatch wiring"\n    )]\n    payload: ProcessLocalSetValuePayload,\n'''
if source.count(old) != 1:
    raise SystemExit(f'expected exactly one pending payload field, found {source.count(old)}')
path.write_text(source.replace(old, new, 1))
PY
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
  crates/control/src/windows_consequential/set_value_http.rs \
  scripts/v43-task8-wire-control-state.py \
  scripts/v43-task8-run-control-state.sh \
  .github/workflows/v43-task8-runner-probe.yml
git diff --cached --check
git commit -m 'feat(v43): bind SetValue state to control lifetime'
git push origin HEAD:feat/v43-windows-set-value-payload-authority
