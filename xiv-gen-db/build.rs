//! Guards the game-data packs against LFS pointer stubs and content-addresses
//! them.
//!
//! The pack URL (`/static/data/<version>/<lang>.rkyv`) and the browser's
//! IndexedDB cache key used to carry the git commit hash
//! (`xiv_gen::data_version()`), so every deploy — game data changed or not —
//! evicted the pack from the edge cache and made every browser re-download
//! it. `XIV_PACK_VERSION_<LANG>` is the SHA-256 of the pack bytes instead, so
//! `<version>` changes exactly when the pack does. It is computed here rather
//! than at runtime so the wasm client (which never reads the packs) and the
//! server agree on it.

use sha2::{Digest, Sha256};

mod lfs_guard {
    include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/lfs_guard.rs"));
}

fn main() {
    // The `embed` feature is what actually pulls these files in via
    // `include_bytes!` in src/lib.rs (the wasm-client build doesn't enable it
    // and never reads xiv-db/*.rkyv at all), so only hard-fail the build here
    // when `embed` is on. Without this gate, the wasm-client crate would
    // refuse to build in any checkout/CI shape that doesn't happen to carry
    // real LFS content for these packs, even though it never needs them.
    let embed_enabled = std::env::var("CARGO_FEATURE_EMBED").is_ok();

    for lang in ["en", "ja", "de", "fr", "cn", "ko", "tc"] {
        let p = format!("{}/../data/xiv-db/{lang}.rkyv", env!("CARGO_MANIFEST_DIR"));
        let path = std::path::Path::new(&p);
        // rustc already tracks `include_bytes!` inputs for recompiling the
        // crate itself, but the build script also needs to re-run its guard
        // check when a pack is swapped out (e.g. LFS pull replaces a pointer
        // stub with real content, or vice versa), so declare it explicitly.
        println!("cargo:rerun-if-changed={p}");
        if embed_enabled {
            lfs_guard::assert_not_lfs_pointer(path);
        }
        let startup = format!(
            "{}/../data/xiv-startup/{lang}.rkyv",
            env!("CARGO_MANIFEST_DIR")
        );
        println!("cargo:rerun-if-changed={startup}");
        let startup = std::path::Path::new(&startup);
        if embed_enabled {
            lfs_guard::assert_not_lfs_pointer(startup);
        }
        println!(
            "cargo:rustc-env=XIV_STARTUP_VERSION_{}={}",
            lang.to_ascii_uppercase(),
            pack_version(startup)
        );
        let version = pack_version(path);
        println!(
            "cargo:rustc-env=XIV_PACK_VERSION_{}={version}",
            lang.to_ascii_uppercase()
        );
    }
}

/// The first 64 bits of the pack's SHA-256, as hex.
///
/// A git-lfs pointer stub carries that same digest (`oid sha256:<hex>` is the
/// SHA-256 of the real content), so a checkout without LFS content — allowed
/// for the wasm client, see `main` — still computes the exact version the
/// server built with. The pointer's own bytes are never hashed: that would
/// silently give the client a URL the server has never heard of.
fn pack_version(path: &std::path::Path) -> String {
    let bytes = std::fs::read(path).unwrap_or_else(|e| {
        panic!(
            "cannot read {}: {e}. The packs live in Git LFS: run `git lfs install && git lfs pull`.",
            path.display()
        )
    });
    let full = if bytes.starts_with(b"version https://git-lfs.github.com/spec") {
        let text = String::from_utf8_lossy(&bytes);
        text.lines()
            .find_map(|line| line.strip_prefix("oid sha256:"))
            .map(|oid| oid.trim().to_string())
            .unwrap_or_else(|| {
                panic!(
                    "{} is a git-lfs pointer without an `oid sha256:` line",
                    path.display()
                )
            })
    } else {
        Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    };
    full[..16].to_string()
}
