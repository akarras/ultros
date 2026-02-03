use heck::ToUpperCamelCase;

fn main() {
    let names = vec!["CompanyCraftType"];
    for name in names {
        let sheet_name = name.to_upper_camel_case();
        println!("Processing: '{}'", sheet_name);
        let (i, c) = sheet_name
            .char_indices()
            .rev()
            .find(|(_i, c)| c.is_uppercase())
            .unwrap();
        println!("Found uppercase '{}' at index {}", c, i);
        let root = &sheet_name[..i];
        println!("Root: '{}'", root);
    }
}
