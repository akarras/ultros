#!/bin/bash
set -e

# prop:href/alt/src/content sets a DOM property after wasm hydration and is a
# no-op during SSR (tachys renders nothing for it), so crawlers/social
# unfurlers never see the value. Use the plain attribute (reactive closures
# work fine on regular attributes) instead of prop:.
if grep -rEn "prop:(href|alt|src|content)=" ultros-frontend/ultros-app/src/; then
    echo "error: found prop:href/alt/src/content in ultros-app/src -- these are SSR no-ops, see above" >&2
    exit 1
fi

# Run cargo fmt check
cargo fmt --all -- --check

# Run cargo clippy check
cargo clippy --all-targets -- -D warnings
