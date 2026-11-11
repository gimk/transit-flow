#!/usr/bin/env bash
# The one check every migration work package runs: format, lints, tests.
set -euo pipefail
cd "$(dirname "$0")/.."
command -v cargo >/dev/null || source "$HOME/.cargo/env"
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
