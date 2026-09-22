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

use anyhow::{Context, Result, anyhow, bail};
use std::ops::Range;
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

/// Longest name kept in the map, in chars. Generic-heavy names reach tens
/// of kilobytes; a trace of such frames would exceed GlitchTip's event size
/// limit, and nothing past the first few hundred chars helps a reader.
pub const MAX_NAME_LEN: usize = 240;

fn is_hex16(s: &str) -> bool {
    s.len() == 16 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Remove every v0 `[<16 hex>]` crate disambiguator (`core[ed30…]::` ->
/// `core::`). A trailing `[N]` wasm-bindgen closure disambiguator is kept.
fn strip_v0_disambiguators(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut rest = name;
    while let Some(open) = rest.find('[') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find(']') {
            Some(close) if is_hex16(&after[..close]) => {
                rest = &after[close + 1..];
            }
            _ => {
                out.push('[');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Drop the trailing `::h<16 hex>` rustc legacy-mangling hash, keeping any
/// wasm-bindgen closure disambiguator (`[N]`) that follows it.
fn strip_legacy_hash(name: &str) -> String {
    let (base, suffix) = match name.rfind('[') {
        Some(i) if name.ends_with(']') => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    let Some(idx) = base.rfind("::h") else {
        return name.to_string();
    };
    if is_hex16(&base[idx + 3..]) {
        format!("{}{}", &base[..idx], suffix)
    } else {
        name.to_string()
    }
}

fn truncate(name: String) -> String {
    match name.char_indices().nth(MAX_NAME_LEN) {
        Some((byte_idx, _)) => {
            let mut out = name[..byte_idx].to_string();
            out.push('…');
            out
        }
        None => name,
    }
}

/// The name as it goes into the map: build-specific hashes removed (so names
/// are stable across builds and the map is a fraction of the size) and
/// capped at [`MAX_NAME_LEN`].
pub fn normalize_name(name: &str) -> String {
    truncate(strip_legacy_hash(&strip_v0_disambiguators(name)))
}

/// Function names from the module's `name` section, index-ascending, with
/// hash suffixes stripped. Errors if the section is absent — that means the
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
                symbols.push((naming.index, normalize_name(naming.name)));
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
