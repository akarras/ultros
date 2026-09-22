//! End-to-end coverage for a `cargo leptos build --split` pkg dir: the main
//! module plus `split___*.wasm` route modules and `chunk_N.wasm` shared
//! modules, each with its own `name` section. Every module must get a
//! `<module>.symbols` sibling (with `.br`/`.gz`), be rewritten without its
//! name section, and have its stale precompressed siblings replaced.

use std::fs;
use std::path::Path;
use wasm_encoder::{
    CodeSection, CustomSection, Function, FunctionSection, Instruction, Module, NameMap,
    NameSection, TypeSection, ValType,
};
use wasm_symbols::{extract_symbols, process_path, process_paths, wasm_modules_in};

fn module(fn_name: Option<&str>) -> Vec<u8> {
    let mut m = Module::new();
    let mut types = TypeSection::new();
    types.ty().function([], [ValType::I32]);
    m.section(&types);
    let mut funcs = FunctionSection::new();
    funcs.function(0);
    m.section(&funcs);
    let mut code = CodeSection::new();
    let mut f = Function::new([]);
    f.instruction(&Instruction::I32Const(1));
    f.instruction(&Instruction::End);
    code.function(&f);
    m.section(&code);
    if let Some(name) = fn_name {
        let mut names = NameSection::new();
        let mut fn_names = NameMap::new();
        fn_names.append(0, name);
        names.functions(&fn_names);
        m.section(&names);
    }
    m.section(&CustomSection {
        name: "producers".into(),
        data: b"rustc".as_slice().into(),
    });
    m.finish()
}

fn tempdir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "wasm-symbols-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// The files cargo-leptos leaves behind: each wasm plus `--precompress`
/// siblings written from the still-named module (stale after the strip).
fn write_pkg(dir: &Path, modules: &[(&str, Vec<u8>)]) {
    for (file, bytes) in modules {
        let path = dir.join(file);
        fs::write(&path, bytes).unwrap();
        fs::write(dir.join(format!("{file}.br")), b"stale").unwrap();
        fs::write(dir.join(format!("{file}.gz")), b"stale").unwrap();
    }
    // Non-wasm neighbours the tool must leave alone.
    fs::write(dir.join("ultros.js"), b"export {}").unwrap();
    fs::write(dir.join("__wasm_split_manifest.json"), b"{}").unwrap();
}

#[test]
fn lists_only_wasm_modules_sorted() {
    let dir = tempdir("list");
    write_pkg(
        &dir,
        &[
            ("ultros.wasm", module(Some("a::h0123456789abcdef"))),
            ("split___analyzer.wasm", module(Some("b"))),
            ("chunk_10.wasm", module(Some("c"))),
            ("chunk_2.wasm", module(Some("d"))),
        ],
    );
    let names: Vec<String> = wasm_modules_in(&dir)
        .unwrap()
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        [
            "chunk_10.wasm",
            "chunk_2.wasm",
            "split___analyzer.wasm",
            "ultros.wasm"
        ]
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn processes_every_module_in_a_split_pkg_dir() {
    let dir = tempdir("dir");
    let main = module(Some("ultros_app::routes::home::h0123456789abcdef"));
    let split = module(Some(
        "ultros_app::routes::analyzer::AnalyzerWorld::{{closure}}",
    ));
    let chunk = module(Some("alloc::raw_vec::RawVec<T>::grow"));
    write_pkg(
        &dir,
        &[
            ("ultros.wasm", main.clone()),
            ("split___analyzer_world.wasm", split.clone()),
            ("chunk_7.wasm", chunk.clone()),
        ],
    );

    let reports = process_path(&dir).unwrap();
    assert_eq!(reports.len(), 3);

    for (file, original, expected) in [
        ("ultros.wasm", &main, "0:ultros_app::routes::home\n"),
        (
            "split___analyzer_world.wasm",
            &split,
            "0:ultros_app::routes::analyzer::AnalyzerWorld::{{closure}}\n",
        ),
        (
            "chunk_7.wasm",
            &chunk,
            "0:alloc::raw_vec::RawVec<T>::grow\n",
        ),
    ] {
        let stem = file.strip_suffix(".wasm").unwrap();
        let symbols = fs::read_to_string(dir.join(format!("{stem}.symbols"))).unwrap();
        assert_eq!(symbols, expected, "{file}: symbol map");
        assert!(
            dir.join(format!("{stem}.symbols.br")).is_file(),
            "{file}: .symbols.br"
        );
        assert!(
            dir.join(format!("{stem}.symbols.gz")).is_file(),
            "{file}: .symbols.gz"
        );

        let stripped = fs::read(dir.join(file)).unwrap();
        assert!(
            stripped.len() < original.len(),
            "{file}: name section stripped"
        );
        assert!(
            extract_symbols(&stripped).is_err(),
            "{file}: shipped module must have no name section"
        );

        // The stale precompressed siblings were replaced with real encodings
        // of the stripped bytes.
        let br = fs::read(dir.join(format!("{file}.br"))).unwrap();
        assert_ne!(br, b"stale");
        let mut out = Vec::new();
        brotli::BrotliDecompress(&mut br.as_slice(), &mut out).unwrap();
        assert_eq!(out, stripped, "{file}: .br decodes to the stripped module");
        let gz = fs::read(dir.join(format!("{file}.gz"))).unwrap();
        assert_ne!(gz, b"stale");
        let mut out = Vec::new();
        std::io::Read::read_to_end(&mut flate2::read::GzDecoder::new(gz.as_slice()), &mut out)
            .unwrap();
        assert_eq!(out, stripped, "{file}: .gz decodes to the stripped module");
    }

    // Neighbours untouched.
    assert_eq!(fs::read(dir.join("ultros.js")).unwrap(), b"export {}");
    assert_eq!(
        fs::read(dir.join("__wasm_split_manifest.json")).unwrap(),
        b"{}"
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_single_file_path_still_works() {
    let dir = tempdir("file");
    write_pkg(&dir, &[("ultros.wasm", module(Some("a::b")))]);
    let reports = process_path(&dir.join("ultros.wasm")).unwrap();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].function_count, 1);
    assert_eq!(
        fs::read_to_string(dir.join("ultros.symbols")).unwrap(),
        "0:a::b\n"
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn one_unnamed_module_fails_the_whole_run() {
    let dir = tempdir("unnamed");
    write_pkg(
        &dir,
        &[
            ("ultros.wasm", module(Some("a::b"))),
            ("chunk_3.wasm", module(None)),
        ],
    );
    let err = process_path(&dir).unwrap_err();
    let msg = format!("{err:#}");
    assert!(
        msg.contains("chunk_3.wasm"),
        "error names the module: {msg}"
    );
    assert!(
        msg.contains("name"),
        "error says the name section is missing: {msg}"
    );
    fs::remove_dir_all(&dir).unwrap();
}

/// What the splitter emits for a chunk that ended up with no functions: a
/// `name` section carrying nothing but an (empty) module name. Real
/// example: `chunk_106.wasm` in the 2026-09-21 build, 183 bytes.
fn empty_chunk() -> Vec<u8> {
    let mut m = Module::new();
    let mut names = NameSection::new();
    names.module("");
    m.section(&names);
    m.section(&CustomSection {
        name: "producers".into(),
        data: b"wasm_split_cli_support".as_slice().into(),
    });
    m.finish()
}

#[test]
fn a_functionless_chunk_gets_an_empty_map_and_is_still_stripped() {
    let dir = tempdir("emptychunk");
    let chunk = empty_chunk();
    write_pkg(
        &dir,
        &[
            ("ultros.wasm", module(Some("a::b"))),
            ("chunk_106.wasm", chunk.clone()),
        ],
    );
    let reports = process_path(&dir).unwrap();
    let report = reports
        .iter()
        .find(|r| r.wasm.ends_with("chunk_106.wasm"))
        .unwrap();
    assert_eq!(report.function_count, 0);
    assert!(report.stripped_len < chunk.len());
    assert_eq!(
        fs::read_to_string(dir.join("chunk_106.symbols")).unwrap(),
        ""
    );
    assert!(extract_symbols(&fs::read(dir.join("chunk_106.wasm")).unwrap()).is_err());
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_run_that_names_nothing_at_all_is_an_error() {
    let dir = tempdir("allempty");
    write_pkg(&dir, &[("ultros.wasm", empty_chunk())]);
    let err = process_path(&dir).unwrap_err();
    assert!(format!("{err:#}").contains("names any function"), "{err:#}");
    // Nothing was written: the strip must not ship without a map.
    assert!(!dir.join("ultros.symbols").exists());
    assert_eq!(fs::read(dir.join("ultros.wasm.br")).unwrap(), b"stale");
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_dir_without_modules_is_an_error() {
    let dir = tempdir("empty");
    fs::write(dir.join("ultros.js"), b"").unwrap();
    let err = process_path(&dir).unwrap_err();
    assert!(format!("{err:#}").contains("no .wasm"), "{err:#}");
    fs::remove_dir_all(&dir).unwrap();
}

/// The Dockerfile shape: the main module named explicitly plus a shell glob
/// that matches it again. It is processed once.
#[test]
fn explicit_module_plus_glob_processes_each_once() {
    let dir = tempdir("dedupe");
    write_pkg(
        &dir,
        &[
            ("ultros.wasm", module(Some("a::b"))),
            ("chunk_1.wasm", module(Some("c::d"))),
        ],
    );
    let reports = process_paths(&[
        dir.join("ultros.wasm"),
        dir.join("chunk_1.wasm"),
        dir.join("ultros.wasm"),
        dir.clone(),
    ])
    .unwrap();
    assert_eq!(reports.len(), 2);
    fs::remove_dir_all(&dir).unwrap();
}
