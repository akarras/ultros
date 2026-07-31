// Worktree-fallback helpers shared by `xiv-gen/src/csv_to_rkyv.rs` and
// `ultros-frontend/ultros-xiv-icons/build.rs`, both via `include!`, so the
// two build paths resolve submodule data dirs with the exact same logic and
// a future edit can't change one without the other.
//
// Constraints that follow from being textually included:
// - std-only, and everything fully qualified — no `use` statements, so this
//   file can't collide with the includer's imports;
// - the tests below compile (and run) only through the xiv-gen include:
//   `cargo test -p xiv-gen --features csv_to_rkyv`. Build scripts never run
//   unit tests, so the icons side is covered by the same tests by virtue of
//   including the same file.

/// Path of the main (first) git worktree, from `git worktree list --porcelain`.
fn main_worktree(cwd: &std::path::Path) -> Option<std::path::PathBuf> {
    let output = std::process::Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(cwd)
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    parse_main_worktree(std::str::from_utf8(&output.stdout).ok()?)
}

fn parse_main_worktree(porcelain: &str) -> Option<std::path::PathBuf> {
    porcelain
        .lines()
        .find_map(|line| line.strip_prefix("worktree "))
        .map(std::path::PathBuf::from)
}

/// `git rev-parse <rev>` in `dir`; `None` on any failure.
fn rev_parse(dir: &std::path::Path, rev: &str) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", rev])
        .current_dir(dir)
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    Some(String::from_utf8(output.stdout).ok()?.trim().to_string())
}

/// Warning to emit when this checkout pins a different commit of `submodule`
/// (path relative to `member_dir`, the directory of the crate that owns it)
/// than the main worktree's `fallback` copy actually has checked out — the
/// fallback would then build against the wrong data. Best-effort: `None` on
/// any git failure or when the pins match; warning-only, never fails a build.
fn pin_drift_warning(
    member_dir: &std::path::Path,
    submodule: &str,
    fallback: &std::path::Path,
) -> Option<String> {
    let pinned = rev_parse(member_dir, &format!("HEAD:./{submodule}"))?;
    let actual = rev_parse(fallback, "HEAD")?;
    (pinned != actual).then(|| drift_message(submodule, &pinned, &actual))
}

fn drift_message(submodule: &str, pinned: &str, actual: &str) -> String {
    format!(
        "{submodule} pin drift: this checkout pins {pinned} but the \
         main worktree has {actual} checked out; building against {actual}"
    )
}

#[cfg(test)]
mod worktree_fallback_tests {
    use super::{drift_message, parse_main_worktree};
    use std::path::Path;

    #[test]
    fn parses_first_worktree_entry() {
        let porcelain = "worktree C:/Users/x/code/ultros\nHEAD abc123\nbranch refs/heads/main\n\n\
                         worktree C:/Users/x/code/ultros/.claude/worktrees/foo\nHEAD def456\n";
        assert_eq!(
            parse_main_worktree(porcelain).as_deref(),
            Some(Path::new("C:/Users/x/code/ultros"))
        );
    }

    #[test]
    fn handles_missing_output() {
        assert_eq!(parse_main_worktree(""), None);
    }

    #[test]
    fn drift_message_names_submodule_and_both_shas() {
        let message = drift_message("ffxiv-datamining", "aaa111", "bbb222");
        assert_eq!(
            message,
            "ffxiv-datamining pin drift: this checkout pins aaa111 but the \
             main worktree has bbb222 checked out; building against bbb222"
        );
    }
}
