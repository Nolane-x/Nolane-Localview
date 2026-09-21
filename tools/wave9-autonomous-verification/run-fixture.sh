#!/usr/bin/env bash
set -euo pipefail

root="$(git rev-parse --show-toplevel)"
cd "$root"

before="$(git status --porcelain=v1)"

cargo test -p localview-contracts --all-targets
cargo test -p localview-verification --all-targets
cargo test -p localview-mutation --all-targets
cargo test -p localview-counterfactual --test shadow_candidate
cargo test -p localview-state-space --all-targets
cargo test -p localview-postcondition-contracts --all-targets
cargo test -p localview-planner --all-targets

after="$(git status --porcelain=v1)"
test "$before" = "$after"
test "$(git worktree list --porcelain | grep '^worktree ' | wc -l)" -eq 1
