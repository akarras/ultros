#!/usr/bin/env bash
# Deterministic Rust regression gate, shared by local checks and GitHub Actions.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

# Full debug information for the SSR test binary can exceed hosted-runner
# resources. Keep assertions and overflow checks enabled while omitting debug
# symbols; callers can opt back in with CARGO_PROFILE_TEST_DEBUG=1 or 2.
export CARGO_PROFILE_TEST_DEBUG="${CARGO_PROFILE_TEST_DEBUG:-0}"

# Run library and binary unit tests, including the recipe analyzer, formula
# evaluator, backend caches, permissions and listing reconciliation. Live DB
# tests already marked #[ignore] remain opt-in. ClickHouse integration targets
# require a separate disposable service and are not counted as unit coverage.
# ultros-client has no authored unit tests and unconditionally enables hydrate.
# Selecting that browser entry point here would unify hydrate with ssr and run
# browser-only APIs in native SSR tests. Keep every ultros-app test; validate
# the client separately with `cargo leptos build` for the WASM target.
cargo test --locked --workspace --exclude universalis --exclude ultros-client --lib --bins

# Universalis mixes deterministic wire-format/status tests with six smoke tests
# against its public API. Only those live tests are excluded from this gate:
# availability and the current contents of Aether must not determine PR status.
# Run all six explicitly with: cargo test --locked -p universalis test::test_
cargo test --locked -p universalis --lib -- \
    --skip test::test_get_worlds \
    --skip test::test_marketboard_multiview_parse \
    --skip test::test_marketboard \
    --skip test::test_history \
    --skip test::test_local_world_history \
    --skip test::test_recently_updated

# This module is absent from default builds. Keep its feature test invocation
# even when a checkout has no tests in the module, so future tests execute.
cargo test --locked -p xiv-gen --features csv_to_rkyv

# Validate the committed game-data pack alongside the pure unit tests.
cargo test --locked -p game-data-pack --test pack_sanity
