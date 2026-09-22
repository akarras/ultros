//! Symbol-map extraction for the release wasm bundle.
//!
//! cargo-leptos runs wasm-opt with `-g` (see `wasm-opt-features` in the root
//! `Cargo.toml`), so the optimized module still carries its `name` custom
//! section. This crate turns that into a `index:name` text map the browser
//! can fetch when a panic happens, and strips the section from the shipped
//! module so the names don't ride along on every page load.
//!
//! The strip is a byte copy of every section except `name`: nothing is
//! re-encoded, so function indices — what the browser's `wasm-function[N]`
//! frames refer to — are exactly those the map was built from.
//!
//! A `--split` build produces several modules — `ultros.wasm` plus
//! `split___*.wasm` route modules and `chunk_N.wasm` shared modules — and
//! browsers report frames against whichever module URL they came from, so
//! every module gets its own `<module>.symbols` sibling ([`process_path`]
//! on the pkg dir).

use anyhow::{Context, Result, anyhow, bail};
use std::fs;
use std::io::Write;
use std::ops::Range;
use std::path::{Path, PathBuf};
use wasmparser::{BinaryReader, Name, NameSectionReader};

const HEADER_LEN: usize = 8;
const CUSTOM_SECTION_ID: u8 = 0;

/// One top-level section of the binary: its id and the byte range of the
/// whole thing (id byte + size LEB + payload), plus where the payload starts.
struct Section {
    id: u8,
    whole: Range<usize>,
    payload: Range<usize>,
}

fn read_leb_u32(bytes: &[u8], pos: &mut usize) -> Result<u32> {
    let mut result: u32 = 0;
    let mut shift = 0;
    loop {
        let byte = *bytes
            .get(*pos)
            .ok_or_else(|| anyhow!("truncated LEB128 at offset {pos}"))?;
        *pos += 1;
        if shift >= 32 {
            bail!("LEB128 too long at offset {pos}");
        }
        result |= u32::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(result);
        }
        shift += 7;
    }
}

fn sections(wasm: &[u8]) -> Result<Vec<Section>> {
    if wasm.len() < HEADER_LEN || &wasm[..4] != b"\0asm" {
        bail!("not a wasm module (bad magic)");
    }
    let mut out = Vec::new();
    let mut pos = HEADER_LEN;
    while pos < wasm.len() {
        let start = pos;
        let id = wasm[pos];
        pos += 1;
        let size = read_leb_u32(wasm, &mut pos)? as usize;
        let payload = pos..pos
            .checked_add(size)
            .ok_or_else(|| anyhow!("section size overflow at offset {start}"))?;
        if payload.end > wasm.len() {
            bail!(
                "section at offset {start} claims {size} bytes but only {} remain",
                wasm.len() - pos
            );
        }
        out.push(Section {
            id,
            whole: start..payload.end,
            payload: payload.clone(),
        });
        pos = payload.end;
    }
    Ok(out)
}

/// For a custom section payload, split off the leading name string and
/// return `(name, data_range)` with the range in module coordinates.
fn custom_section_name<'a>(wasm: &'a [u8], payload: &Range<usize>) -> Result<(&'a str, usize)> {
    let mut pos = payload.start;
    let len = read_leb_u32(wasm, &mut pos)? as usize;
    let end = pos + len;
    if end > payload.end {
        bail!("custom section name overruns its section");
    }
    let name = std::str::from_utf8(&wasm[pos..end]).context("custom section name is not UTF-8")?;
    Ok((name, end))
}

fn is_name_section(wasm: &[u8], section: &Section) -> Result<bool> {
    if section.id != CUSTOM_SECTION_ID {
        return Ok(false);
    }
    Ok(custom_section_name(wasm, &section.payload)?.0 == "name")
}

/// Drop the trailing `::h<16 hex>` rustc legacy-mangling hash, keeping any
/// wasm-bindgen closure disambiguator (`[N]`) that follows it. Names are
/// then stable across builds and the map is smaller.
pub fn strip_hash_suffix(name: &str) -> String {
    let (base, suffix) = match name.rfind('[') {
        Some(i) if name.ends_with(']') => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    let Some(idx) = base.rfind("::h") else {
        return name.to_string();
    };
    let hash = &base[idx + 3..];
    if hash.len() == 16 && hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        format!("{}{}", &base[..idx], suffix)
    } else {
        name.to_string()
    }
}

/// Whether the module carries a `name` custom section at all.
pub fn has_name_section(wasm: &[u8]) -> Result<bool> {
    for section in sections(wasm)? {
        if is_name_section(wasm, &section)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The name a human wants to read in a stack trace. `cargo leptos build
/// --split` passes `--no-demangle` to wasm-bindgen (the splitter keys off
/// mangled names), so the section carries raw `_R…` v0 / `_ZN…` legacy
/// symbols; a non-split build carries wasm-bindgen's already-demangled
/// legacy form. Both are reduced to the same shape: a Rust path with no
/// crate-disambiguator / `::h<hash>` noise (`{:#}` on the demangler), and
/// any trailing wasm-bindgen `[N]` disambiguator kept. Anything that is not
/// a Rust symbol (`__wbindgen_malloc`, `fimport$3`) passes through.
pub fn readable_name(name: &str) -> String {
    let (base, suffix) = match name.rfind('[') {
        Some(i) if name.ends_with(']') => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    match rustc_demangle::try_demangle(base) {
        Ok(demangled) => format!("{demangled:#}{suffix}"),
        Err(_) => strip_hash_suffix(name),
    }
}

/// Function names from the module's `name` section, index-ascending, made
/// readable with [`readable_name`]. Errors if the section is absent — that means the
/// `-g` plumbing in the build regressed and must not ship silently.
pub fn extract_symbols(wasm: &[u8]) -> Result<Vec<(u32, String)>> {
    let section = sections(wasm)?
        .into_iter()
        .find(|s| is_name_section(wasm, s).unwrap_or(false))
        .ok_or_else(|| anyhow!("module has no `name` custom section"))?;
    let (_, data_start) = custom_section_name(wasm, &section.payload)?;
    let data = &wasm[data_start..section.payload.end];
    let reader = NameSectionReader::new(BinaryReader::new(data, data_start as u64));
    let mut symbols = Vec::new();
    for subsection in reader {
        if let Name::Function(map) = subsection.context("malformed name subsection")? {
            for naming in map {
                let naming = naming.context("malformed function name entry")?;
                symbols.push((naming.index, readable_name(naming.name)));
            }
        }
    }
    symbols.sort_by_key(|(index, _)| *index);
    Ok(symbols)
}

/// `index:name\n` per function — trivially parseable in the browser.
pub fn format_symbols(symbols: &[(u32, String)]) -> String {
    let mut out = String::with_capacity(symbols.len() * 64);
    for (index, name) in symbols {
        out.push_str(&index.to_string());
        out.push(':');
        out.push_str(name);
        out.push('\n');
    }
    out
}

/// The module with its `name` custom section removed and every other byte
/// copied verbatim.
pub fn strip_name_section(wasm: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(wasm.len());
    out.extend_from_slice(&wasm[..HEADER_LEN]);
    for section in sections(wasm)? {
        if is_name_section(wasm, &section)? {
            continue;
        }
        out.extend_from_slice(&wasm[section.whole]);
    }
    Ok(out)
}

/// What [`process_path`] did to one module, for the build log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleReport {
    pub wasm: PathBuf,
    pub symbols: PathBuf,
    pub function_count: usize,
    pub original_len: usize,
    pub stripped_len: usize,
}

/// Every `*.wasm` directly inside `dir`, sorted by file name so runs are
/// reproducible. Precompressed siblings (`.wasm.br`, `.wasm.gz`) are not
/// modules and are skipped.
pub fn wasm_modules_in(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut modules: Vec<PathBuf> = fs::read_dir(dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == "wasm"))
        .collect();
    modules.sort();
    Ok(modules)
}

/// `dir/name.ext` -> `dir/name.ext.<suffix>` (so `ultros.wasm` ->
/// `ultros.wasm.br`, matching what cargo-leptos `--precompress` writes).
fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(".");
    name.push(suffix);
    path.with_file_name(name)
}

/// Write `bytes` to `path` and refresh its `.br` / `.gz` siblings. Same
/// quality/window cargo-leptos's `--precompress` uses (brotli 11, lgwin 22),
/// so the rewritten module's `.br` is as small as the one it replaces. The
/// server's ServeDir picks the precompressed variants up as-is, so a stale
/// sibling would silently ship the pre-strip bytes.
fn write_with_precompressed(path: &Path, bytes: &[u8]) -> Result<()> {
    fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))?;

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
    let br_path = sibling(path, "br");
    fs::write(&br_path, &br).with_context(|| format!("writing {}", br_path.display()))?;

    let mut gz = flate2::write::GzEncoder::new(
        Vec::with_capacity(bytes.len() / 3),
        flate2::Compression::best(),
    );
    gz.write_all(bytes)?;
    let gz = gz.finish()?;
    let gz_path = sibling(path, "gz");
    fs::write(&gz_path, &gz).with_context(|| format!("writing {}", gz_path.display()))?;
    Ok(())
}

struct Extracted {
    wasm_path: PathBuf,
    wasm: Vec<u8>,
    symbols: Vec<(u32, String)>,
}

/// Read a module and pull its function names. Errors name the module so a
/// failure in a 100-chunk directory points at the right file. An empty
/// map is allowed here: the splitter emits the odd chunk with a `name`
/// section but no functions at all (`chunk_106.wasm`, 183 bytes, in the
/// first split build), and those must not fail the run.
fn extract_module(wasm_path: &Path) -> Result<Extracted> {
    let wasm = fs::read(wasm_path).with_context(|| format!("reading {}", wasm_path.display()))?;
    let symbols = extract_symbols(&wasm)
        .with_context(|| format!("{}: extracting function names", wasm_path.display()))?;
    Ok(Extracted {
        wasm_path: wasm_path.to_path_buf(),
        wasm,
        symbols,
    })
}

/// Process one module or, for a directory, every `*.wasm` in it: write
/// `<module>.symbols` (+ `.br`/`.gz`), rewrite the module without its `name`
/// section and refresh its precompressed siblings.
///
/// Every module is read and its names extracted before anything is written,
/// so a module missing its name section — or a run that would produce no
/// names at all, which means the `-g` plumbing regressed — fails without
/// leaving the directory half-processed. Non-`.wasm` files are never
/// touched.
pub fn process_path(path: &Path) -> Result<Vec<ModuleReport>> {
    process_paths(&[path])
}

/// [`process_path`] over several arguments, each a module or a directory.
/// Duplicates (a shell glob that also matched an explicitly named module)
/// are processed once — stripping twice would find no name section.
pub fn process_paths(paths: &[impl AsRef<Path>]) -> Result<Vec<ModuleReport>> {
    let mut modules: Vec<PathBuf> = Vec::new();
    for path in paths {
        let path = path.as_ref();
        if path.is_dir() {
            let found = wasm_modules_in(path)?;
            if found.is_empty() {
                bail!("no .wasm modules in {}", path.display());
            }
            modules.extend(found);
        } else {
            modules.push(path.to_path_buf());
        }
    }
    if modules.is_empty() {
        bail!("no .wasm modules given");
    }
    let mut seen = std::collections::HashSet::new();
    modules.retain(|m| seen.insert(m.clone()));

    let extracted = modules
        .iter()
        .map(|p| extract_module(p))
        .collect::<Result<Vec<_>>>()?;
    if extracted.iter().all(|e| e.symbols.is_empty()) {
        bail!("no module names any function (was wasm-opt run with `-g`?)");
    }

    let mut reports = Vec::with_capacity(extracted.len());
    for Extracted {
        wasm_path,
        wasm,
        symbols,
    } in extracted
    {
        let symbols_path = wasm_path.with_extension("symbols");
        let text = format_symbols(&symbols);
        write_with_precompressed(&symbols_path, text.as_bytes())?;

        let stripped = strip_name_section(&wasm)
            .with_context(|| format!("{}: stripping name section", wasm_path.display()))?;
        write_with_precompressed(&wasm_path, &stripped)?;

        reports.push(ModuleReport {
            wasm: wasm_path,
            symbols: symbols_path,
            function_count: symbols.len(),
            original_len: wasm.len(),
            stripped_len: stripped.len(),
        });
    }
    Ok(reports)
}
