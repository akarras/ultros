use std::process::Command;

// Emit GIT_HASH for use via `env!("GIT_HASH")`. Falls back to "dirty" when git
// is unavailable, the working tree isn't a real git checkout (worktree pointer
// files don't resolve inside containers, archives have no .git, etc.), or
// `git rev-parse` otherwise fails. Replaces the `git-const` proc-macro which
// hard-panics in those cases.
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let git_hash = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "dirty".to_string());
    println!("cargo:rustc-env=GIT_HASH={git_hash}");
}
