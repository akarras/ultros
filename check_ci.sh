#!/bin/bash
set -e

# Run cargo fmt check
cargo fmt --all -- --check

# Run cargo clippy check
cargo clippy --all-targets -- -D warnings
