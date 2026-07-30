mod manifest;

mod lfs_guard {
    include!("../../data/lfs_guard.rs");
}

fn main() {}

#[cfg(test)]
mod lfs_guard_tests {
    use super::lfs_guard::assert_not_lfs_pointer;
    use std::io::Write;

    #[test]
    fn panics_on_lfs_pointer_stub() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("pointer.bin");
        let mut f = std::fs::File::create(&path).expect("create pointer file");
        write!(
            f,
            "version https://git-lfs.github.com/spec/v1\noid sha256:0000000000000000000000000000000000000000000000000000000000000000\nsize 12345\n"
        )
        .expect("write pointer contents");
        drop(f);

        let result = std::panic::catch_unwind(|| assert_not_lfs_pointer(&path));
        let err = result.expect_err("expected a panic for an lfs pointer stub");
        let message = err
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| err.downcast_ref::<&str>().map(|s| s.to_string()))
            .expect("panic payload should be a string");
        assert!(
            message.contains("git-lfs pointer"),
            "unexpected panic message: {message}"
        );
        assert!(
            message.contains("git lfs install && git lfs pull"),
            "unexpected panic message: {message}"
        );
    }

    #[test]
    fn passes_on_real_bytes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("real.bin");
        std::fs::write(&path, [0xDEu8, 0xAD, 0xBE, 0xEF, 1, 2, 3, 4]).expect("write real file");

        let result = std::panic::catch_unwind(|| assert_not_lfs_pointer(&path));
        assert!(result.is_ok(), "did not expect a panic for a real file");
    }
}
