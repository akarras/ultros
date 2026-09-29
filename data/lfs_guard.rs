/// Errors when `path` contains a git-lfs pointer stub instead of real data.
/// Build scripts include! this file. Kept dependency-free.
pub fn assert_not_lfs_pointer(path: &std::path::Path) {
    let mut head = [0u8; 42];
    use std::io::Read;
    let n = std::fs::File::open(path)
        .and_then(|mut f| f.read(&mut head))
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    if head[..n].starts_with(b"version https://git-lfs.github.com/spec") {
        panic!(
            "{} is a git-lfs pointer, not the real file. Run: git lfs install && git lfs pull",
            path.display()
        );
    }
}
