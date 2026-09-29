// Generate typed measurements from the actual wire schema. New tables/columns
// automatically enter the audit; an unsupported shape fails the build instead
// of silently disappearing from its inventory.
use std::{collections::BTreeMap, env, fs, path::PathBuf};
use syn::{GenericArgument, Item, PathArguments, Type};

fn type_args(ty: &Type, expected: &str) -> Vec<Type> {
    type_args_of(ty, &[expected])
}

/// Like `type_args`, for a position that may hold any of `expected`.
fn type_args_of(ty: &Type, expected: &[&str]) -> Vec<Type> {
    let Type::Path(path) = ty else {
        panic!("expected one of {expected:?}")
    };
    let segment = path.path.segments.last().unwrap();
    assert!(
        expected.iter().any(|name| segment.ident == name),
        "expected one of {expected:?}, found {}",
        segment.ident
    );
    let PathArguments::AngleBracketed(args) = &segment.arguments else {
        panic!("expected generic arguments for {expected:?}")
    };
    args.args
        .iter()
        .map(|arg| match arg {
            GenericArgument::Type(ty) => ty.clone(),
            _ => panic!("expected type argument"),
        })
        .collect()
}

fn type_name(ty: &Type) -> String {
    let Type::Path(path) = ty else {
        panic!("expected named row type")
    };
    path.path.segments.last().unwrap().ident.to_string()
}

fn main() {
    let schema = "../xiv-gen/src/lib.rs";
    println!("cargo:rerun-if-changed={schema}");
    let source = fs::read_to_string(schema).unwrap();
    let ast = syn::parse_file(&source).unwrap();
    let structs: BTreeMap<_, _> = ast
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Struct(item) => Some((item.ident.to_string(), item)),
            _ => None,
        })
        .collect();
    let data = structs.get("Data").unwrap();
    let mut generated = String::from(
        "fn inventory(data: &xiv_gen::Data, options: &Options) -> anyhow::Result<Vec<Measurement>> { let mut rows = Vec::new();\n",
    );
    for table in &data.fields {
        let name = table.ident.as_ref().unwrap().to_string();
        // Tables are `IdMap`s (row-id order, see xiv-gen/src/id_map.rs); a
        // plain `HashMap` is still accepted so a new table can land either way.
        let types = type_args_of(&table.ty, &["IdMap", "HashMap"]);
        let value = &types[1];
        let vector = type_name(value) == "Vec";
        let row_type = if vector {
            type_args(value, "Vec")[0].clone()
        } else {
            value.clone()
        };
        let row_name = type_name(&row_type);
        generated.push_str(&format!("if options.includes_table({name:?}) {{\nlet table = &data.{name};\nrows.push(measure({name:?}, \"table\", table.len(), &rkyv::to_bytes::<_, 256>(table).map_err(|e| anyhow::anyhow!(\"{{e:?}}\"))?, options.quality)?);\n"));
        if let Some(row) = structs.get(&row_name) {
            // Sort keys first so columns have stable row order. Subrow vectors
            // retain their wire order. Include the key alongside each value.
            generated.push_str("if options.columns { let mut entries: Vec<_> = table.iter().collect(); entries.sort_by_key(|(key, _)| key.0);\n");
            for field in &row.fields {
                let column = field.ident.as_ref().unwrap().to_string();
                let expression = if vector {
                    format!(
                        "entries.iter().flat_map(|(key, values)| values.iter().map(move |row| (key.0, snapshot(&row.{column})))).collect::<Vec<_>>()"
                    )
                } else {
                    format!(
                        "entries.iter().map(|(key, row)| (key.0, snapshot(&row.{column}))).collect::<Vec<_>>()"
                    )
                };
                generated.push_str(&format!("{{ let values = {expression}; rows.push(measure({:?}, \"column_sample\", values.len(), &rkyv::to_bytes::<_, 256>(&values).map_err(|e| anyhow::anyhow!(\"{{e:?}}\"))?, options.quality)?); }}\n", format!("{name}.{column}")));
            }
            generated.push_str("}\n");
        } else {
            // Reverse indexes are maps of identifier vectors, not row structs.
            assert!(
                vector && row_name.ends_with("Id"),
                "unknown row type {row_name}"
            );
        }
        generated.push_str("}\n");
    }
    generated.push_str("Ok(rows) }\n");
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("inventory.rs"),
        generated,
    )
    .unwrap();
}
