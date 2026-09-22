//! Builds a tiny module with `wasm-encoder`, then checks that the tool
//! extracts the expected symbol map and strips exactly the `name` section,
//! leaving every other byte untouched.

use wasm_encoder::{
    CodeSection, CustomSection, Function, FunctionSection, Instruction, Module, NameMap,
    NameSection, TypeSection, ValType,
};
use wasm_symbols::{extract_symbols, format_symbols, strip_hash_suffix, strip_name_section};

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
            "ultros_app::routes::item_view::ItemView::{{closure}}::h0123456789abcdef",
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
fn hash_suffix_only_stripped_when_it_is_a_rustc_hash() {
    assert_eq!(strip_hash_suffix("a::b::h0123456789abcdef"), "a::b");
    // wasm-bindgen's closure disambiguator comes after the hash.
    assert_eq!(strip_hash_suffix("a::b::h0123456789abcdef[3]"), "a::b[3]");
    assert_eq!(strip_hash_suffix("a::b::hello"), "a::b::hello");
    assert_eq!(strip_hash_suffix("__wbindgen_malloc"), "__wbindgen_malloc");
    assert_eq!(strip_hash_suffix("a::b::h0123"), "a::b::h0123");
}
