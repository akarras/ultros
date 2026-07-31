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
    }
}
