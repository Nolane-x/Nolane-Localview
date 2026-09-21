# Wave 9 autonomous verification tools

This directory contains deterministic developer entry points for the Wave 9 verification lane.

`run-fixture.sh` runs only local Rust tests. It does not access external services, launch an externally reachable server, mutate the checked-out project, persist reports, commit or push.

The counterfactual integration fixture creates its own temporary git repository and disposable detached worktree, applies a bounded text patch there, proves the fixture's dirty real worktree remains unchanged, and removes the worktree before exit.

The GitHub workflow `.github/workflows/wave9-autonomous-verification.yml` is the authoritative CI gate.
