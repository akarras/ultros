use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=Cargo.toml");

    emit_git_hash();
}

// Emit GIT_HASH for use via `env!("GIT_HASH")`. Falls back to "dirty" when git
// is unavailable, the working tree isn't a real git checkout (worktree pointer
// files don't resolve inside containers, archives have no .git, etc.), or
// `git rev-parse` otherwise fails. Replaces the `git-const` proc-macro which
// hard-panics in those cases.
fn emit_git_hash() {
    rerun_when_head_moves();
    let git_hash =
        git_stdout(&["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "dirty".to_string());
    println!("cargo:rustc-env=GIT_HASH={git_hash}");
}

// The hash must track commits, so rerun when HEAD or the reflog changes.
// `--git-path` resolves inside worktrees; when git is unavailable nothing is
// emitted and the "dirty" fallback behaves as before.
fn rerun_when_head_moves() {
    for name in ["HEAD", "logs/HEAD"] {
        if let Some(path) = git_stdout(&["rev-parse", "--git-path", name]) {
            println!("cargo:rerun-if-changed={path}");
        }
    }
}

fn git_stdout(args: &[&str]) -> Option<String> {
    Command::new("git")
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}
