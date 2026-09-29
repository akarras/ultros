//! `wasm-symbols <pkg-dir>/ultros.wasm`
//!
//! Run in the Docker builder right after `cargo leptos build --release
//! --precompress`. Writes `ultros.symbols` next to the module, rewrites the
//! module without its `name` section, and regenerates the `.br` / `.gz`
//! siblings of both files (cargo-leptos wrote the wasm's from the still-named
//! module, so they are stale). The server's `/pkg/<hash>/` ServeDir picks the
//! precompressed variants up as-is.

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

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let (Some(wasm_path), None) = (args.next(), args.next()) else {
        bail!("usage: wasm-symbols <path/to/ultros.wasm>");
    };
    let wasm_path = Path::new(&wasm_path);
    let wasm = fs::read(wasm_path).with_context(|| format!("reading {}", wasm_path.display()))?;

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
    println!("{} functions named", symbols.len());
    write_compressed(&symbols_path, symbols_text.as_bytes())?;

    let stripped = wasm_symbols::strip_name_section(&wasm)?;
    fs::write(wasm_path, &stripped)
        .with_context(|| format!("rewriting {}", wasm_path.display()))?;
    println!(
        "stripped name section: {} -> {} bytes",
        wasm.len(),
        stripped.len()
    );
    write_compressed(wasm_path, &stripped)?;
    Ok(())
}
