//! Zero-allocation case-insensitive string operations.
//! These functions are designed to avoid allocating a new `String` (e.g. via `to_lowercase()`)
//! in hot paths like filtering lists.

/// Checks if `haystack` contains `needle_lower`, ignoring ASCII case.
/// `needle_lower` must be pre-converted to lowercase.
pub fn contains_ignore_ascii_case(haystack: &str, needle_lower: &str) -> bool {
    let haystack = haystack.as_bytes();
    let needle = needle_lower.as_bytes();

    if needle.is_empty() {
        return true;
    }

    haystack
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle))
}

/// Checks if `haystack` starts with `needle_lower`, ignoring ASCII case.
/// `needle_lower` must be pre-converted to lowercase.
pub fn starts_with_ignore_ascii_case(haystack: &str, needle_lower: &str) -> bool {
    let haystack = haystack.as_bytes();
    let needle = needle_lower.as_bytes();

    if needle.is_empty() {
        return true;
    }
    if haystack.len() < needle.len() {
        return false;
    }

    haystack[..needle.len()].eq_ignore_ascii_case(needle)
}
