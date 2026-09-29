#!/usr/bin/env bash
#
# Measure the shipped client bundle without a full `cargo leptos build`.
#
#   ./scripts/measure_wasm_size.sh <label>
#
# Builds `ultros-client` for wasm32 under the `wasm-release` profile, then runs
# the same post-processing cargo-leptos does — wasm-bindgen, wasm-opt -Oz,
# brotli -q 11 — and prints the sizes at each stage. Artifacts land in
# `target/wasm-size/<label>/` so two labels can be compared directly.
#
# This reproduces cargo-leptos's numbers exactly while skipping the server half
# of the build, which makes A/B size comparisons practical: a full frontend
# build is ~8 minutes and rebuilding the server adds several more for a number
# that does not change.
#
# The wasm-bindgen CLI must match the `wasm-bindgen` crate version in
# Cargo.lock, or the generated glue will not load.

set -euo pipefail

if [[ $# -ne 1 ]]; then
    echo "usage: $0 <label>" >&2
    exit 2
fi

LABEL="$1"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="$ROOT/target/wasm-size/$LABEL"
SRC="$ROOT/target/wasm32-unknown-unknown/wasm-release/ultros_client.wasm"

# cargo-leptos downloads its own binaryen; reuse it so the optimiser version
# matches what CI and production ship.
WASM_OPT="$(find "${HOME}/.cache/cargo-leptos" -type f -name wasm-opt 2>/dev/null | sort | tail -1)"
if [[ -z "$WASM_OPT" ]]; then
    echo "wasm-opt not found under ~/.cache/cargo-leptos — run 'cargo leptos build' once to fetch it" >&2
    exit 1
fi

expected_bindgen="$(awk '/^name = "wasm-bindgen"$/ {getline; gsub(/[";]/, ""); print $3; exit}' "$ROOT/Cargo.lock")"
actual_bindgen="$(wasm-bindgen --version | awk '{print $2}')"
if [[ "$expected_bindgen" != "$actual_bindgen" ]]; then
    echo "wasm-bindgen CLI is $actual_bindgen but Cargo.lock pins $expected_bindgen" >&2
    echo "install the matching CLI: cargo install -f wasm-bindgen-cli --version $expected_bindgen" >&2
    exit 1
fi

cd "$ROOT"
cargo build --profile wasm-release -p ultros-client --lib \
    --target wasm32-unknown-unknown --no-default-features --features hydrate

rm -rf "$OUT"
mkdir -p "$OUT"
wasm-bindgen --target web --out-dir "$OUT" --out-name ultros --no-typescript "$SRC"
"$WASM_OPT" -Oz --enable-bulk-memory --enable-nontrapping-float-to-int \
    -o "$OUT/ultros_bg.opt.wasm" "$OUT/ultros_bg.wasm"
brotli -q 11 -f -k "$OUT/ultros_bg.opt.wasm"

printf '%s\n' "$LABEL"
printf '  cargo output : %12d\n' "$(stat -c%s "$SRC")"
printf '  wasm-bindgen : %12d\n' "$(stat -c%s "$OUT/ultros_bg.wasm")"
printf '  wasm-opt -Oz : %12d\n' "$(stat -c%s "$OUT/ultros_bg.opt.wasm")"
printf '  brotli -q 11 : %12d\n' "$(stat -c%s "$OUT/ultros_bg.opt.wasm.br")"
