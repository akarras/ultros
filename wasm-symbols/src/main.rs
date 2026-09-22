//! `wasm-symbols <pkg-dir | module.wasm>...`
//!
//! Run in the Docker builder right after `cargo leptos build --release
//! --split --precompress` and before `scripts/post_split.sh` nests the pkg
//! under `pkg/<hash>/`. For every `*.wasm` in the dir — the main module,
//! the `split___*.wasm` route modules and the `chunk_N.wasm` shared modules
//! — it writes `<module>.symbols` next to the module, rewrites the module
//! without its `name` section, and regenerates the `.br` / `.gz` siblings of
//! both files (cargo-leptos wrote the wasm's from the still-named module, so
//! they are stale). The server's `/pkg/<hash>/` ServeDir picks the
//! precompressed variants up as-is.

use anyhow::{Result, bail};
use std::path::PathBuf;

fn main() -> Result<()> {
    let paths: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    if paths.is_empty() {
        bail!("usage: wasm-symbols <path/to/pkg-dir | path/to/module.wasm>...");
    }
    let reports = wasm_symbols::process_paths(&paths)?;
    let mut total_saved = 0usize;
    for r in &reports {
        total_saved += r.original_len - r.stripped_len;
        println!(
            "{}: {} functions named -> {}; stripped name section: {} -> {} bytes",
            r.wasm.display(),
            r.function_count,
            r.symbols.display(),
            r.original_len,
            r.stripped_len
        );
    }
    println!(
        "{} module(s) processed, {} bytes of name sections stripped",
        reports.len(),
        total_saved
    );
    Ok(())
}
