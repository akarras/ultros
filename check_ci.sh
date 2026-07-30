#!/bin/bash
set -e

# Run cargo fmt check
cargo fmt --all -- --check

# Run cargo clippy check
cargo clippy --all-targets -- -D warnings

# xiv-gen's csv_to_rkyv module (and its tests) only compile under this
# non-default feature, so the workspace clippy above never sees it. Lint it
# explicitly; the matching test gate is:
#   cargo test -p xiv-gen --features csv_to_rkyv
# (CI's test step is disabled, so run that locally when touching the module.)
cargo clippy -p xiv-gen --features csv_to_rkyv --all-targets -- -D warnings
