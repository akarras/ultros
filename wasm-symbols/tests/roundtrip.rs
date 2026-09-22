//! Builds a tiny module with `wasm-encoder`, then checks that the tool
//! extracts the expected symbol map and strips exactly the `name` section,
//! leaving every other byte untouched.

use wasm_encoder::{
    CodeSection, CustomSection, Function, FunctionSection, Instruction, Module, NameMap,
    NameSection, TypeSection, ValType,
};
use wasm_symbols::{
    MAX_NAME_LEN, extract_symbols, format_symbols, normalize_name, strip_name_section,
};

/// Two functions; the first is unnamed to exercise sparse indices. The
/// `producers` custom section sits *after* `name` to prove stripping is
/// selective rather than "drop everything from the name section onward".
fn module(with_names: bool) -> Vec<u8> {
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
    if with_names {
        let mut names = NameSection::new();
        let mut fn_names = NameMap::new();
        fn_names.append(
            1,
            "ultros_app[1729e4642c0fad2d]::routes::item_view::ItemView::{{closure}}",
        );
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

#[test]
fn strip_removes_only_the_name_section() {
    let stripped = strip_name_section(&module(true)).unwrap();
    assert_eq!(stripped, module(false));
    // Idempotent: a module without a name section passes through unchanged.
    assert_eq!(strip_name_section(&stripped).unwrap(), stripped);
}

#[test]
fn legacy_hash_suffix_only_stripped_when_it_is_a_rustc_hash() {
    assert_eq!(normalize_name("a::b::h0123456789abcdef"), "a::b");
    // wasm-bindgen's closure disambiguator comes after the hash.
    assert_eq!(normalize_name("a::b::h0123456789abcdef[3]"), "a::b[3]");
    assert_eq!(normalize_name("a::b::hello"), "a::b::hello");
    assert_eq!(normalize_name("__wbindgen_malloc"), "__wbindgen_malloc");
    assert_eq!(normalize_name("a::b::h0123"), "a::b::h0123");
}

/// v0 mangling (the default on current nightlies) demangles with a
/// `[<16 hex>]` crate disambiguator after every crate name. They are noise
/// for a human, shift on every dependency bump, and are the bulk of the map.
#[test]
fn v0_crate_disambiguators_are_removed_everywhere() {
    assert_eq!(
        normalize_name(
            "<ultros_app[1729e4642c0fad2d]::routes::item_view::ItemView as core[ed30c1ab14c18277]::ops::function::FnOnce<()>>::call_once"
        ),
        "<ultros_app::routes::item_view::ItemView as core::ops::function::FnOnce<()>>::call_once"
    );
    // A trailing `[N]` closure disambiguator is not a crate hash.
    assert_eq!(
        normalize_name("ultros_app[1729e4642c0fad2d]::f::{{closure}}[2]"),
        "ultros_app::f::{{closure}}[2]"
    );
    // Only exactly 16 lowercase hex digits qualify.
    assert_eq!(normalize_name("a[0123]::b"), "a[0123]::b");
}

/// Fully generic names run to tens of kilobytes (a `VirtualGrid`
/// instantiation with all its type arguments). Fifty such frames would
/// blow GlitchTip's event size limit, so names are capped; the prefix is
/// the informative part.
#[test]
fn over_long_names_are_truncated_at_a_char_boundary() {
    let long = format!("ultros_app::grid::{}", "é".repeat(MAX_NAME_LEN));
    let out = normalize_name(&long);
    assert!(out.ends_with('…'), "{out}");
    assert!(out.chars().count() <= MAX_NAME_LEN + 1);
    assert!(out.starts_with("ultros_app::grid::é"));
    let exact = "x".repeat(MAX_NAME_LEN);
    assert_eq!(normalize_name(&exact), exact);
}
