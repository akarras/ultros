mod lfs_guard {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../data/lfs_guard.rs"
    ));
}

fn main() {
    // This crate is not feature-gated the way xiv-gen-db is — every consumer
    // that links ultros-xiv-icons needs real icon bytes, so the guard always
    // hard-fails on a pointer stub.
    let p = format!(
        "{}/../../data/icons/images.tar.zst",
        env!("CARGO_MANIFEST_DIR")
    );
    println!("cargo:rerun-if-changed={p}");
    lfs_guard::assert_not_lfs_pointer(std::path::Path::new(&p));
}
