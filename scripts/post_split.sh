#!/usr/bin/env bash
# Post-process a `cargo leptos build --split` bundle so the server can serve it
# from its hashed URL (`/pkg/<git hash>/`, see `create_leptos_app` in
# `ultros/src/leptos.rs`).
#
# Two things cargo-leptos 0.3 assumes that Ultros does not do:
#
# 1. The generated split loader (`__wasm_split.*.js`) imports the main module
#    by the absolute path `/pkg/ultros.js`. We serve the bundle under
#    `/pkg/<hash>/`, and a second JS module instance at the bare path would
#    try to initialise a second wasm instance. Rewrite the import to be
#    relative — the loader sits next to `ultros.js`.
#
# 2. `HydrationScripts` looks for `__wasm_split_manifest.json` under
#    `<site_root>/<site_pkg_dir>`, and `site_pkg_dir` is overridden at runtime
#    to `pkg/<hash>`. Move the built pkg dir to that path so the manifest is
#    found and SSR responses carry `<link rel=preload>` for the current
#    route's chunks. The server falls back to the unhashed dir when the hashed
#    one is absent, so running this script is optional for local dev.
#
# Usage: scripts/post_split.sh [site_root]   (default: target/site)
# Idempotent: re-running after a rebuild moves the fresh pkg into place again.
set -euo pipefail

site_root="${1:-target/site}"
pkg="$site_root/pkg"

if [[ ! -d "$pkg" ]]; then
    echo "post_split: $pkg does not exist — run 'cargo leptos build' first" >&2
    exit 1
fi

# Same derivation as `ultros/build.rs` (`env!("GIT_HASH")`).
git_hash="$(git rev-parse --short HEAD 2>/dev/null || true)"
git_hash="${git_hash:-dirty}"

# A previous run may already have nested an older build; drop it so the move
# below never lands inside it. The fresh output lives at the top level of $pkg.
rm -rf "$pkg/$git_hash"

shopt -s nullglob
loaders=("$pkg"/__wasm_split*.js)
if (( ${#loaders[@]} == 0 )); then
    echo "post_split: no __wasm_split*.js in $pkg (built without --split?) — only relocating" >&2
fi
for loader in "${loaders[@]}"; do
    # Only the main-module import is absolute; chunk URLs are already
    # `import.meta.url`-relative.
    sed -i 's#from "/pkg/ultros\.js"#from "./ultros.js"#' "$loader"
    if grep -q 'from "/pkg/' "$loader"; then
        echo "post_split: unexpected absolute /pkg/ import left in $loader" >&2
        exit 1
    fi
    # `--precompress` ran before us; refresh the siblings for the edited file.
    for f in "$loader.br" "$loader.gz"; do
        rm -f "$f"
    done
    if command -v brotli >/dev/null 2>&1; then
        brotli -q 11 -f -o "$loader.br" "$loader"
    fi
    if command -v gzip >/dev/null 2>&1; then
        gzip -9 -k -f "$loader"
    fi
done

tmp="$(mktemp -d "$site_root/pkg.XXXXXX")"
mv "$pkg"/* "$tmp"/
mv "$tmp" "$pkg/$git_hash"
echo "post_split: bundle now at $pkg/$git_hash"
ls -la "$pkg/$git_hash" | grep -E '\.(wasm|js|json)$' || true
