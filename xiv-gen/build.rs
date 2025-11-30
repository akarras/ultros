use codegen_rs::{Field, Function, Impl, Module, Scope, Struct};

use heck::{ToSnakeCase, ToUpperCamelCase};
use lazy_static::lazy_static;
use regex::Regex;
use std::borrow::Cow;
use std::collections::HashSet;
use std::env;
use std::fs::write;

use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug)]
struct Args {
    // Whether to descend into subdirectories in the ffxiv-data-mining table
    recurse_directories: bool,

    // bin_code_generation: bool,
    /// List filter
    list_filter: Vec<String>,

    /// Parent data struct for all data types
    ///
    /// generated example:
    /// ```
    /// #[derive(Default, Debug)]
    /// struct Data {
    ///    recipes: HashMap<RecipeId, Recipe>,
    ///    items: HashMap<ItemId, Item>
    /// }
    ///
    /// impl Data {
    ///   fn set_recipes(&mut self, recipes: HashMap<RecipeId, Recipe>) {
    ///
    ///   }
    /// }
    /// ```
    db: Struct,
    db_impl: Impl,

    /// Contains the code to create the db. Only exists in local binary
    ///
    /// generated example:
    /// ```
    /// fn convert_csv(csv: &str) -> _ {
    ///     // parses csv and converts
    /// }
    ///
    /// fn read_data() -> Data {
    ///   Data {
    ///     recipes: convert_csv("Recipe.csv")
    ///   }
    /// }
    /// ```
    read_data: Function,
}

#[derive(Debug, Default)]
struct RequestedStructData {
    requested_struct: String,
    sample_data: String,
}

#[derive(Debug, Default)]
struct ScopeData {
    /// List of known structs
    known_structs: HashSet<String>,
    /// List of unknown data types paired with suggested data types to ease parsing
    requested_structs: Vec<RequestedStructData>,
}

fn apply_derives(s: &mut Struct) -> &mut Struct {
    s.derive("Debug")
        .derive("Clone")
        .derive("Serialize")
        .derive("Deserialize")
        .derive("PartialEq")
        .derive("Encode")
        .derive("Decode")
}

/// Feed in a column, detect all the data. pronto muchacho.
#[derive(Debug)]
enum DataDetector {
    Unresolved {
        int_range: Option<(i64, i64)>,
        // the column that this detector is represented by
        column: usize,
    },
    Detected(DataType),
}

#[derive(Debug)]
enum DataType {
    String,
    UnsignedInt8,
    UnsignedInt16,
    UnsignedInt32,
    UnsignedInt64,
    SignedInt8,
    SignedInt16,
    SignedInt32,
    SignedInt64,
    Float,
    Bool,
    /// Represents a key that indexes into another sheet
    /// In the form of int.int, such that {otherkey}.{rownumber}
    ReferenceKey,
}

impl DataType {
    fn field_type(&self, sheet_name: &str) -> String {
        match self {
            DataType::String => "String".to_string(),
            DataType::UnsignedInt8 => "u8".to_string(),
            DataType::UnsignedInt16 => "u16".to_string(),
            DataType::UnsignedInt32 => "u32".to_string(),
            DataType::UnsignedInt64 => "u64".to_string(),
            DataType::SignedInt8 => "i8".to_string(),
            DataType::SignedInt16 => "i16".to_string(),
            DataType::SignedInt32 => "i32".to_string(),
            DataType::SignedInt64 => "i64".to_string(),
            DataType::Float => "f32".to_string(),
            DataType::Bool => "bool".to_string(),
            DataType::ReferenceKey => {
                // Should render the parent sheet name + "Key"
                // GilShopItem -> GilShopId
                let (i, _c) = sheet_name
                    .char_indices()
                    .rev()
                    .find(|(_i, c)| c.is_uppercase())
                    .unwrap();
                let root = &sheet_name[..i];
                format!("SubrowKey<{root}Id>")
            }
        }
    }
}

impl DataDetector {
    fn new(column: usize) -> Self {
        DataDetector::Unresolved {
            int_range: None,
            column,
        }
    }

    fn next_record(&mut self, record: &str) {
        if let DataDetector::Detected(_) = self {
            return;
        }
        if record == "TRUE" || record == "FALSE" {
            *self = DataDetector::Detected(DataType::Bool);
        }
        lazy_static! {
            // regex: check is number
            static ref RE: Regex = Regex::new(r"^(\+|-|)[0-9]+\.[0-9]*$").unwrap();
        }

        if RE.is_match(record) {
            match *self {
                DataDetector::Unresolved { column, .. } => {
                    if column == 0 {
                        *self = DataDetector::Detected(DataType::ReferenceKey);
                        return;
                    } else {
                        *self = DataDetector::Detected(DataType::Float);
                        return;
                    }
                }
                DataDetector::Detected(_) => {
                    return;
                }
            }
        }
        if record.chars().any(|a| !a.is_numeric()) {
            *self = DataDetector::Detected(DataType::String)
        }
        if record.is_empty() {
            return;
        }
        if let DataDetector::Unresolved { int_range, .. } = self {
            let value = record.parse::<i64>().unwrap();
            if let Some((min, max)) = int_range {
                *max = (*max).max(value);
                *min = (*min).min(value);
            } else {
                *int_range = Some((value, value));
            }
        }
    }

    fn end(self) -> DataType {
        // assume this is an int range if we haven't returned any other data types.
        match self {
            DataDetector::Unresolved { int_range, column } => {
                if column == 0 {
                    return DataType::SignedInt32;
                }
                if let Some((min, max)) = int_range {
                    // start small and expand the range.
                    [
                        (0..=1, DataType::Bool),
                        (u8::MIN as i64..=u8::MAX as i64, DataType::UnsignedInt8),
                        (i8::MIN as i64..=i8::MAX as i64, DataType::SignedInt8),
                        (u16::MIN as i64..=u16::MAX as i64, DataType::UnsignedInt16),
                        (i16::MIN as i64..=i16::MAX as i64, DataType::SignedInt16),
                        (u32::MIN as i64..=u32::MAX as i64, DataType::UnsignedInt32),
                        (i32::MIN as i64..=i32::MAX as i64, DataType::SignedInt32),
                        (u64::MIN as i64..=u64::MAX as i64, DataType::UnsignedInt64),
                        (i64::MIN..=i64::MAX, DataType::SignedInt64),
                    ]
                    .into_iter()
                    .find(|(range, _data_type)| range.contains(&min) && range.contains(&max))
                    .map(|(_, d)| d)
                    .unwrap()
                } else {
                    DataType::String
                }
            }
            DataDetector::Detected(d) => d,
        }
    }
}

fn create_struct(
    csv_name: &str,
    path: &str,
    args: &mut Args,
    scope: &mut impl Container,
    local_data: &mut ScopeData,
) {
    println!("reading {csv_name}");
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .from_path(path)
        .expect("unable to open path");
    let mut records = reader.records();
    let _line_one = records
        .next()
        .expect("First line not found")
        .expect("Reader error on first line");
    let line_two = records
        .next()
        .expect("Second line not found")
        .expect("Error reading second line");
    let line_three = records
        .next()
        .expect("Third line not found")
        .expect("Third line error reading");
    // iterate over all columns
    let mut line_four: Vec<_> = records
        .next()
        .map(|m| {
            m.unwrap()
                .iter()
                .enumerate()
                .map(|(col, m)| {
                    let mut data = DataDetector::new(col);
                    data.next_record(m);
                    data
                })
                .collect()
        })
        .unwrap();
    // read the entire csv and determine a datatype
    records.for_each(|s| {
        s.unwrap()
            .iter()
            .zip(line_four.iter_mut())
            .for_each(|(record, detector)| {
                detector.next_record(record);
            })
    });
    let line_four: Vec<_> = line_four.into_iter().map(|m| m.end()).collect();
    let csv_name = &csv_name.to_upper_camel_case();
    let key_name = format!("{}Id", csv_name);
    let mut s = Struct::new(csv_name);
    let i = Impl::new(csv_name);
    apply_derives(&mut s).vis("pub");
    let mut parse_this_function = None;
    let mut pk = None;
    let mut unknown_counter = 0;
    let fields: Vec<(String, String)> = line_two
        .iter()
        .zip(line_three.iter())
        .zip(line_four.iter())
        .map(|((field_name, field_value), sample_data)| {
            let mut line_one = if field_name == "#" {
                "key_id".to_string()
            } else if field_name.is_empty() {
                unknown_counter += 1;
                format!("unknown_{}", unknown_counter)
            } else {
                field_name
                    .replace('{', "_")
                    .replace('}', "")
                    .replace('[', "_")
                    .replace(']', "")
                    .replace("PvP", "Pvp")
                    .to_snake_case()
            };
            if line_one == "type" {
                line_one = "r#type".to_string();
            } else if line_one == "trait" {
                line_one = "r#trait".to_string();
            } else if line_one == "move" {
                line_one = "r#move".to_string();
            } else if line_one.chars().next().unwrap_or_default().is_ascii_digit() {
                line_one = format!("num{line_one}");
            }

            lazy_static! {
                // regex: check is int type
                static ref INT: Regex = Regex::new(r#"^(u|)int(8|16|32|64)$"#).unwrap();
                // regex: check is bit offset
                static ref BIT: Regex = Regex::new(r#"^bit(&[0-9]+|)|bool$"#).unwrap();
            }
            if BIT.is_match(field_value) {
                (line_one, "bool".to_string())
            } else if INT.is_match(field_value) {
                let mut line_two = field_value.replace("int", "");
                // uint64 -> u64
                // int64 -> 64, add the i if no u
                if !line_two.starts_with('u') {
                    line_two = format!("i{}", line_two);
                }

                if line_one == "key_id" {
                    let mut key = Struct::new(&key_name);
                    apply_derives(&mut key).derive("FromStr").derive("Default").derive("Hash").derive("Eq").derive("Copy").vis("pub").tuple_field(sample_data.field_type(csv_name)).vis("pub");
                    scope.push_struct(key);

                    line_two = key_name.clone();
                    let db_field_name = format!("{}s", csv_name.to_snake_case());
                    let key_value = match sample_data {
                        DataType::ReferenceKey => {
                            Cow::from(format!("Vec<{csv_name}>"))
                        },
                        _ => {
                            Cow::Borrowed(csv_name.as_str())
                        }
                    };
                    let db_field_key = match sample_data {
                        DataType::ReferenceKey => {
                            let (index, _) = csv_name.char_indices().rev().find(|(_i, c)| c.is_uppercase()).unwrap();
                            let parent_key = &csv_name[..index];
                            format!("HashMap<{parent_key}Id, {key_value}>")
                        },
                        _ => {
                            format!("HashMap<{key_name}, {key_value}>")
                        }
                    };
                    args.db
                        .field(&db_field_name, &db_field_key).vis("pub");
                    pk = Some(db_field_name.to_string());
                    match sample_data {
                        DataType::ReferenceKey => {
                            parse_this_function = Some(format!("{db_field_name}: read_csv::<{csv_name}>(r#\"{path}\"#).into_iter().fold(HashMap::new(), |mut map, m| {{ map.entry(m.key_id.0.0).or_default().push(m); map }}),"));
                        },
                        _ => {
                            parse_this_function = Some(format!("{db_field_name}: read_csv::<{csv_name}>(r#\"{path}\"#).into_iter().map(|m| (m.key_id, m)).collect(),"));
                        }
                    }
                    local_data.known_structs.insert(key_name.clone());
                }
                (line_one, line_two)
            } else if field_value == "byte" {
                (line_one, "u8".to_string())
            } else if field_value == "sbyte" {
                (line_one, "i8".to_string())
            } else if field_value == "str" {
                (line_one, "String".to_string())
            } else {
                // remove trailing numbers from the field_name before adding the ID
                let field_name = field_name.to_upper_camel_case();
                let field_name: String =
                    field_name.chars().filter(|c| !c.is_numeric()).collect();
                if field_name.is_empty() {
                    (line_one, "String".to_string())
                } else {
                    let local_key_name = format!("{}Id", field_value.to_upper_camel_case());
                    if !local_data
                        .requested_structs
                        .iter()
                        .any(|d| d.requested_struct == local_key_name)
                    {
                        local_data.requested_structs.push(RequestedStructData {
                            requested_struct: local_key_name.clone(),
                            sample_data: sample_data.field_type(csv_name),
                        });
                    }
                    (line_one, local_key_name)
                }
            }
        })
        .collect();
    if fields.len() > 100 {
        // handle hugeee fields?
        #[derive(Debug)]
        enum KeyType {
            Normal,
            /// Where usize = max i
            Single(usize),
        }

        let mut root_names = Vec::new();
        for (key, value) in &fields {
            lazy_static! {
                // regex: check is number
                static ref DOUBLE: Regex = Regex::new(r#"([A-z_])+([0-9]+)_([0-9]+)"#).unwrap();
                static ref SINGLE: Regex = Regex::new(r#"([A-z_])+([0-9]+)"#).unwrap();
            }
            if let Some(captures) = DOUBLE.captures(key) {
                let key_1 = captures.get(2).unwrap();
                let key_2 = captures.get(3).unwrap();
                // let root = captures.get(0).unwrap();
                let root = &key[..key_1.start() - 1];
                let root = format!("{}_{}", root, key_2.as_str().parse::<usize>().unwrap());
                let key = KeyType::Single(key_1.as_str().parse().unwrap());
                if let Some((_, (k, _), _)) = root_names.iter_mut().find(|(key, _, _)| key == &root)
                {
                    *k = key;
                } else {
                    root_names.push((root, (key, value), 0));
                }
            } else if let Some(captures) = SINGLE.captures(key) {
                let key_1 = captures.get(2).unwrap();
                let root = &key[..key_1.start() - 1];
                if root == "unknown" {
                    let (_, _, skip) = root_names.last_mut().unwrap();
                    *skip += 1;
                    continue;
                }
                let key = KeyType::Single(key_1.as_str().parse().unwrap());
                if let Some((_, (k, _), _)) = root_names.iter_mut().find(|(key, _, _)| key == root)
                {
                    *k = key;
                } else {
                    root_names.push((root.to_string(), (key, value), 0));
                }
            } else {
                root_names.push((key.as_str().to_string(), (KeyType::Normal, value), 0));
            }
        }
        for (name, (multi, datatype), skip) in root_names.iter() {
            match multi {
                KeyType::Normal => {
                    let mut field = Field::new(name, datatype.as_str());
                    field.vis("pub");
                    s.push_field(field);
                }
                KeyType::Single(count) => {
                    let mut field = Field::new(name, format!("Vec<{datatype}>"));
                    let count = *count + 1;
                    let skip = *skip;
                    field
                        .annotation(vec![&format!(
                            "#[dumb_csv(count = {count}, skip = {skip})]"
                        )])
                        .vis("pub");
                    s.push_field(field);
                }
            }
        }
        s.derive("DumbCsvDeserialize");
        let pk = pk.unwrap();
        parse_this_function = Some(format!(
            "{pk}: read_dumb_csv::<{csv_name}>(r#\"{path}\"#).into_iter().map(|m| (m.key_id, m)).collect(),"
        ))
        // panic!("{root_names:?}");
    } else {
        for (field_name, field_value) in fields.iter() {
            //let mut function = Function::new(&format!("get_{}", field_name.replace('#', "")));
            //function
            //    .vis("pub")
            //    .arg_ref_self()
            //    .line(format!("self.{field_name}.clone()"))
            //    .ret(field_value);
            //i.push_fn(function);
            let mut field = Field::new(field_name, field_value).vis("pub").to_owned();
            if field_value == "i64" {
                field.annotation(vec![
                    "#[serde(deserialize_with = \"deserialize_i64_from_u8_array\")]",
                ]);
            }
            if field_value == "bool" {
                field.annotation(vec![
                    "#[serde(deserialize_with = \"deserialize_bool_from_anything_custom\")]",
                ]);
            }
            if field_value.ends_with("Id") {
                field.annotation(vec![r#"#[serde(deserialize_with = "ok_or_default")]"#]);
            }
            s.push_field(field);
        }
    }
    let function = parse_this_function.unwrap();
    args.read_data.line(function);

    scope.push_struct(s);
    scope.push_impl(i);
}

trait Container {
    fn push_struct(&mut self, str: Struct) -> &mut Self;
    fn push_module(&mut self, module: Module) -> &mut Self;
    fn push_impl(&mut self, i: Impl) -> &mut Self;
}

impl Container for Module {
    fn push_struct(&mut self, str: Struct) -> &mut Self {
        self.push_struct(str)
    }

    fn push_module(&mut self, module: Module) -> &mut Self {
        self.push_module(module)
    }

    fn push_impl(&mut self, i: Impl) -> &mut Self {
        self.push_impl(i)
    }
}

impl Container for Scope {
    fn push_struct(&mut self, str: Struct) -> &mut Self {
        self.push_struct(str)
    }

    fn push_module(&mut self, module: Module) -> &mut Self {
        self.push_module(module)
    }

    fn push_impl(&mut self, i: Impl) -> &mut Self {
        self.push_impl(i)
    }
}

/// Note: ScopeOrModule could be replaced with a trait, but I am lazy
fn read_dir<T: Container>(path: PathBuf, mut scope: T, args: &mut Args) -> T {
    // Dirs = a module scope
    // keep track of all the structs we add in this scope
    let mut local_data = ScopeData::default();
    let path = std::fs::read_dir(path).expect("Unable to open dir");
    for result in path {
        let file = result.expect("Result not found in scope");
        if let Ok(ft) = file.file_type() {
            if ft.is_dir() && args.recurse_directories {
                let file_name = file.file_name();
                let file_name = file_name.to_str().unwrap();
                let module = Module::new(file_name);
                let module = read_dir(file.path(), module, args);
                scope.push_module(module);
            } else if ft.is_file()
                && let Some(ext) = file.path().extension()
                && ext == "csv"
            {
                let file_name = file.file_name();
                let mut file_name = file_name.to_str().unwrap().split('.');
                let file_name = file_name.next().unwrap().to_string();
                if !args.list_filter.contains(&file_name) {
                    continue;
                }
                let path = file.path();
                create_struct(
                    &file_name,
                    path.to_str().unwrap(),
                    args,
                    &mut scope,
                    &mut local_data,
                );
            }
        }
    }
    // requested_structs - known_structs
    // create structs for remaining requested
    for RequestedStructData {
        requested_struct,
        sample_data,
    } in local_data.requested_structs.into_iter().filter(
        |RequestedStructData {
             requested_struct,
             sample_data: _,
         }| !local_data.known_structs.contains(requested_struct),
    ) {
        let mut s = Struct::new(&requested_struct);
        apply_derives(&mut s)
            .vis("pub")
            .derive("FromStr")
            .derive("Default")
            .tuple_field(sample_data)
            .vis("pub");
        scope.push_struct(s);
    }
    scope
}

fn get_table_names(path: impl AsRef<Path>) -> Box<dyn Iterator<Item = (String, String)>> {
    let dir = std::fs::read_dir(path).unwrap();
    Box::new(
        dir.into_iter()
            .flat_map(|m| {
                let entry = m.unwrap();
                let value: Option<Box<dyn Iterator<Item = (String, String)>>> =
                    if entry.file_type().unwrap().is_dir() {
                        // get_table_names(entry.path()).into_iter()
                        None
                    } else {
                        let csv_name = entry.file_name().into_string().unwrap().replace(".csv", "");
                        let feature_name = csv_name.to_snake_case();
                        Some(Box::new([(csv_name, feature_name)].into_iter()))
                    };
                value
            })
            .flatten(),
    )
}

fn main() {
    // figure out what features have been enabled
    let dir = "./ffxiv-datamining/csv/";
    let mut table_names: Vec<_> = get_table_names(dir).collect();
    table_names.sort();
    let mut list = table_names
        .iter()
        .map(|(_, feature_name)| format!("{} = []", feature_name))
        .collect::<Vec<String>>();
    list.sort();
    let list_str = list.join("\n");
    let all_features_str = format!(
        "all = [{}]",
        table_names
            .iter()
            .map(|(_, feature_name)| format!("\"{feature_name}\""))
            .collect::<Vec<String>>()
            .join(",")
    );
    println!("available features: \n{}\n{}", all_features_str, list_str);
    let list_filter: Vec<_> = table_names
        .into_iter()
        .flat_map(|(csv_name, feature)| {
            env::var(format!("CARGO_FEATURE_{}", feature.to_uppercase())).map(|_| csv_name)
        })
        .collect();
    write(
        "./extra.toml",
        format!("{}\n{}", all_features_str, list_str).as_bytes(),
    )
    .unwrap();

    let mut args: Args = Args {
        recurse_directories: false,
        // bin_code_generation: true,
        list_filter,
        db: Struct::new("Data"),
        db_impl: Impl::new("Data"),
        read_data: Function::new("read_data"),
    };

    // Start the read function with the data header
    args.read_data.line("Data {");
    args.recurse_directories = false;
    let out_dir = env::var_os("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("types.rs");
    let path = std::fs::canonicalize(dir).unwrap();
    let scope = Scope::new();
    let mut scope = read_dir(path, scope, &mut args);
    apply_derives(&mut args.db).vis("pub").derive("Default");
    scope.push_struct(args.db);
    scope.push_impl(args.db_impl);
    scope.import("std::collections", "HashMap");
    scope.import("crate::subrow_key", "SubrowKey");
    scope.import("derive_more", "FromStr");
    scope.import("dumb_csv", "DumbCsvDeserialize");
    write(dest_path, scope.to_string()).unwrap();

    let conversion_files = Path::new(&out_dir).join("deserialization.rs");

    args.read_data.line("}").ret("Data").vis("pub");

    let mut ser_scope = Scope::new();
    ser_scope.push_fn(args.read_data);
    write(conversion_files, ser_scope.to_string()).unwrap();

    println!("cargo:rerun-if-changed=build.rs");
    // note: add error checking yourself.
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    let git_hash = String::from_utf8(output.stdout).unwrap();
    println!("cargo:rustc-env=GIT_HASH={}", git_hash);
}
