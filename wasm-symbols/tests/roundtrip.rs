//! Builds a tiny module with `wasm-encoder`, then checks that the tool
//! extracts the expected symbol map and strips exactly the `name` section,
//! leaving every other byte untouched.

use wasm_encoder::{
    CodeSection, CustomSection, Function, FunctionSection, Instruction, Module, NameMap,
    NameSection, TypeSection, ValType,
};
use wasm_symbols::{
    extract_symbols, format_symbols, has_name_section, readable_name, strip_hash_suffix,
    strip_name_section,
};

/// Two functions; the first is unnamed to exercise sparse indices. The
/// `producers` custom section sits *after* `name` to prove stripping is
/// selective rather than "drop everything from the name section onward".
fn module(with_names: bool) -> Vec<u8> {
    module_with(
        with_names
            .then_some("ultros_app::routes::item_view::ItemView::{{closure}}::h0123456789abcdef"),
    )
}

fn module_named(name: &str) -> Vec<u8> {
    module_with(Some(name))
}

fn module_with(fn_name: Option<&str>) -> Vec<u8> {
    let mut m = Module::new();
    let mut types = TypeSection::new();
    types.ty().function([], [ValType::I32]);
    m.section(&types);
    let mut funcs = FunctionSection::new();
    funcs.function(0);
    funcs.function(0);
    m.section(&funcs);
    let mut code = CodeSection::new();
    for v in [1, 2] {
        let mut f = Function::new([]);
        f.instruction(&Instruction::I32Const(v));
        f.instruction(&Instruction::End);
        code.function(&f);
    }
    m.section(&code);
    if let Some(name) = fn_name {
        let mut names = NameSection::new();
        let mut fn_names = NameMap::new();
        fn_names.append(1, name);
        names.functions(&fn_names);
        m.section(&names);
    }
    m.section(&CustomSection {
        name: "producers".into(),
        data: b"rustc".as_slice().into(),
    });
    m.finish()
}

#[test]
fn extracts_function_names_without_hash_suffix() {
    let symbols = extract_symbols(&module(true)).unwrap();
    assert_eq!(
        symbols,
        vec![(
            1,
            "ultros_app::routes::item_view::ItemView::{{closure}}".to_string()
        )]
    );
    assert_eq!(
        format_symbols(&symbols),
        "1:ultros_app::routes::item_view::ItemView::{{closure}}\n"
    );
}

#[test]
fn missing_name_section_is_an_error() {
    let err = extract_symbols(&module(false)).unwrap_err();
    assert!(
        err.to_string().contains("name"),
        "error should say the name section is missing: {err}"
    );
}

/// Split chunks may lack a `name` section; the CLI skips those instead of
/// failing the build, so the check has to be a clean boolean.
#[test]
fn has_name_section_reports_presence() {
    assert!(has_name_section(&module(true)).unwrap());
    assert!(!has_name_section(&module(false)).unwrap());
}

/// A data-only split chunk carries a `name` section with no function
/// subsection. That is an empty map, not a malformed module, and the section
/// still has to be stripped.
#[test]
fn empty_name_section_is_an_empty_map_and_still_stripped() {
    let mut m = Module::new();
    let mut names = NameSection::new();
    names.module("chunk");
    m.section(&names);
    let wasm = m.finish();
    assert!(has_name_section(&wasm).unwrap());
    assert!(extract_symbols(&wasm).unwrap().is_empty());
    let stripped = strip_name_section(&wasm).unwrap();
    assert!(!has_name_section(&stripped).unwrap());
    assert_eq!(stripped, Module::new().finish());
}

#[test]
fn strip_removes_only_the_name_section() {
    let stripped = strip_name_section(&module(true)).unwrap();
    assert_eq!(stripped, module(false));
    // Idempotent: a module without a name section passes through unchanged.
    assert_eq!(strip_name_section(&stripped).unwrap(), stripped);
}

#[test]
fn hash_suffix_only_stripped_when_it_is_a_rustc_hash() {
    assert_eq!(strip_hash_suffix("a::b::h0123456789abcdef"), "a::b");
    // wasm-bindgen's closure disambiguator comes after the hash.
    assert_eq!(strip_hash_suffix("a::b::h0123456789abcdef[3]"), "a::b[3]");
    assert_eq!(strip_hash_suffix("a::b::hello"), "a::b::hello");
    assert_eq!(strip_hash_suffix("__wbindgen_malloc"), "__wbindgen_malloc");
    assert_eq!(strip_hash_suffix("a::b::h0123"), "a::b::h0123");
}

/// `cargo leptos build --split` passes `--no-demangle` to wasm-bindgen (the
/// splitter keys off mangled names), so the name section carries raw v0
/// symbols. The map must still read like Rust paths, without the crate
/// disambiguator hashes, or `in_app`/machinery matching in the browser
/// never fires. Fixtures are real entries from the 2026-09-21 split build.
#[test]
fn v0_mangled_names_are_demangled_without_hashes() {
    assert_eq!(
        readable_name(
            "_RNCNvNtNtCseUETqJg6HxX_10ultros_app6routes9item_view29___component_item_view_contents8_0B7_"
        ),
        "ultros_app::routes::item_view::__component_item_view_content::{closure#10}"
    );
    assert_eq!(
        readable_name("_RNvNtCsfOrqGARnJ9R_4core9panicking18panic_bounds_check"),
        "core::panicking::panic_bounds_check"
    );
}

#[test]
fn legacy_and_unmangled_names_keep_the_old_treatment() {
    // Legacy mangling (a non-split build): demangled, `::h<hash>` dropped.
    assert_eq!(
        readable_name("_ZN10ultros_app6routes9item_view8ItemView17h0123456789abcdefE"),
        "ultros_app::routes::item_view::ItemView"
    );
    // Already-demangled legacy output with wasm-bindgen's closure
    // disambiguator after the hash.
    assert_eq!(readable_name("a::b::h0123456789abcdef[3]"), "a::b[3]");
    // Not Rust at all: passed through.
    assert_eq!(readable_name("__wbindgen_malloc"), "__wbindgen_malloc");
    assert_eq!(readable_name("fimport$99"), "fimport$99");
}

#[test]
fn extracts_v0_names_demangled() {
    let symbols = extract_symbols(&module_named(
        "_RNvNtCsfOrqGARnJ9R_4core9panicking18panic_bounds_check",
    ))
    .unwrap();
    assert_eq!(
        symbols,
        vec![(1, "core::panicking::panic_bounds_check".to_string())]
    );
}
