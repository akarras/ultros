use leptos_i18n_build::{Config, ParseOptions, TranslationsInfos};
use std::error::Error;
use std::path::PathBuf;
use std::process::Command;

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=Cargo.toml");

    emit_git_hash();

    let i18n_mod_directory = PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("i18n");

    let cfg = Config::new("en")?
        .add_locale("fr")?
        .add_locale("de")?
        .add_locale("ja")?
        .add_locale("cn")?
        .add_locale("tc")?
        .add_locale("ko")?
        .parse_options(ParseOptions::new().interpolate_display(true));

    let translations_infos = TranslationsInfos::parse(cfg)?;

    translations_infos.emit_diagnostics();
    translations_infos.rerun_if_locales_changed();
    translations_infos.generate_i18n_module(i18n_mod_directory)?;

    Ok(())
}

// Emit GIT_HASH for use via `env!("GIT_HASH")`. Falls back to "dirty" when git
// is unavailable, the working tree isn't a real git checkout (worktree pointer
// files don't resolve inside containers, archives have no .git, etc.), or
// `git rev-parse` otherwise fails. Replaces the `git-const` proc-macro which
// hard-panics in those cases.
fn emit_git_hash() {
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
