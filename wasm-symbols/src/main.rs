//! `wasm-symbols <pkg-dir>/ultros.wasm [other.wasm ...]`
//!
//! Run in the Docker builder right after `cargo leptos build --release
//! --split --precompress` and before `post_split.sh` relocates the bundle.
//! For each module it writes `<name>.symbols` next to the module, rewrites the
//! module without its `name` section, and regenerates the `.br` / `.gz`
//! siblings of both files (cargo-leptos wrote the wasm's from the still-named
//! module, so they are stale). The server's `/pkg/<hash>/` ServeDir picks the
//! precompressed variants up as-is.
//!
//! The first path is the main module and must carry a `name` section — a
//! wasm with no map must not ship silently. The remaining paths are the
//! `--split` chunks: cargo-leptos runs the same wasm-opt over them, so they
//! carry names too and would otherwise ship them on every first visit to a
//! lazy route. A chunk without a `name` section is skipped, not an error.

use anyhow::{Context, Result, bail};
use std::fs;
use std::io::Write;
use std::path::Path;

fn write_compressed(path: &Path, bytes: &[u8]) -> Result<()> {
    let br_path = path.with_extension(format!(
        "{}.br",
        path.extension().and_then(|e| e.to_str()).unwrap_or("")
    ));
    let gz_path = path.with_extension(format!(
        "{}.gz",
        path.extension().and_then(|e| e.to_str()).unwrap_or("")
    ));

    // Same quality/window cargo-leptos's `--precompress` uses (brotli 11,
    // lgwin 22), so the rewritten wasm's `.br` is as small as the one it
    // replaces.
    let mut br = Vec::with_capacity(bytes.len() / 4);
    {
        let params = brotli::enc::BrotliEncoderParams {
            quality: 11,
            lgwin: 22,
            ..Default::default()
        };
        let mut writer = brotli::CompressorWriter::with_params(&mut br, 1 << 16, &params);
        writer.write_all(bytes)?;
        writer.flush()?;
    }
    fs::write(&br_path, &br).with_context(|| format!("writing {}", br_path.display()))?;

    let mut gz = flate2::write::GzEncoder::new(
        Vec::with_capacity(bytes.len() / 3),
        flate2::Compression::best(),
    );
    gz.write_all(bytes)?;
    let gz = gz.finish()?;
    fs::write(&gz_path, &gz).with_context(|| format!("writing {}", gz_path.display()))?;

    println!(
        "{}: {} raw, {} br, {} gz",
        path.display(),
        bytes.len(),
        br.len(),
        gz.len()
    );
    Ok(())
}

/// Extract, strip and recompress one module. `Ok(false)` means the module
/// has no `name` section and was left untouched.
fn process(wasm_path: &Path) -> Result<bool> {
    let wasm = fs::read(wasm_path).with_context(|| format!("reading {}", wasm_path.display()))?;

    if !wasm_symbols::has_name_section(&wasm)
        .with_context(|| format!("{}: reading sections", wasm_path.display()))?
    {
        return Ok(false);
    }
    let symbols = wasm_symbols::extract_symbols(&wasm)
        .with_context(|| format!("{}: extracting function names", wasm_path.display()))?;
    if symbols.is_empty() {
        bail!(
            "{}: name section has no function names",
            wasm_path.display()
        );
    }
    let symbols_path = wasm_path.with_extension("symbols");
    let symbols_text = wasm_symbols::format_symbols(&symbols);
    fs::write(&symbols_path, &symbols_text)
        .with_context(|| format!("writing {}", symbols_path.display()))?;
    println!("{}: {} functions named", wasm_path.display(), symbols.len());
    write_compressed(&symbols_path, symbols_text.as_bytes())?;

    let stripped = wasm_symbols::strip_name_section(&wasm)?;
    fs::write(wasm_path, &stripped)
        .with_context(|| format!("rewriting {}", wasm_path.display()))?;
    println!(
        "{}: stripped name section: {} -> {} bytes",
        wasm_path.display(),
        wasm.len(),
        stripped.len()
    );
    write_compressed(wasm_path, &stripped)?;
    Ok(true)
}

fn main() -> Result<()> {
    let paths: Vec<_> = std::env::args_os().skip(1).collect();
    let Some((main_module, chunks)) = paths.split_first() else {
        bail!("usage: wasm-symbols <path/to/ultros.wasm> [chunk.wasm ...]");
    };

    let main_module = Path::new(main_module);
    if !process(main_module)? {
        bail!(
            "{}: module has no `name` custom section (was wasm-opt run with `-g`?)",
            main_module.display()
        );
    }

    let mut skipped = 0usize;
    for chunk in chunks {
        let chunk = Path::new(chunk);
        if chunk == main_module {
            continue;
        }
        if !process(chunk)? {
            skipped += 1;
        }
    }
    if skipped > 0 {
        println!("{skipped} module(s) had no name section and were left as-is");
    }
    Ok(())
}
