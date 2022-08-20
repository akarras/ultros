use clap::Parser;
use codegen::{Field, Function, Impl, Module, Scope, Struct, Trait};
use csv::Reader;
use heck::{ToLowerCamelCase, ToSnakeCase, ToUpperCamelCase};
use lazy_static::lazy_static;
use regex::Regex;
use std::collections::HashSet;
use std::env;
use std::fmt::{Display, Formatter};
use std::fs::{write, File};
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::str::FromStr;

#[derive(Debug)]
struct Args {
    /// Number of times to greet
    recurse_directories: bool,

    /// Number of times to greet
    bin_code_generation: bool,

    /// List filter
    list_filter: Option<Vec<String>>,

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
        .derive("Encode")
        .derive("Decode")
}

/// Feed in a column, detect all the data. pronto muchacho.
#[derive(Debug)]
enum DataDetector {
    Unresolved { int_range: Option<(i64, i64)> },
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
}

impl Display for DataType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                DataType::String => "String",
                DataType::UnsignedInt8 => "u8",
                DataType::UnsignedInt16 => "u16",
                DataType::UnsignedInt32 => "u32",
                DataType::UnsignedInt64 => "u64",
                DataType::SignedInt8 => "i8",
                DataType::SignedInt16 => "i16",
                DataType::SignedInt32 => "i32",
                DataType::SignedInt64 => "i64",
                DataType::Float => "f32",
                DataType::Bool => "bool",
            }
        )
    }
}

impl DataDetector {
    fn new() -> Self {
        DataDetector::Unresolved { int_range: None }
    }

    fn next_record(&mut self, record: &str) {
        if record == "TRUE" || record == "FALSE" {
            *self = DataDetector::Detected(DataType::Bool);
        }
        lazy_static! {
            // regex: check is number
            static ref RE: Regex = Regex::new(r#"^(\+|-|)[0-9]+\.[0-9]*$"#).unwrap();
        }

        if RE.is_match(record) {
            *self = DataDetector::Detected(DataType::Float);
        }
        if record.chars().any(|a| !a.is_numeric()) {
            *self = DataDetector::Detected(DataType::String)
        }
        if record.is_empty() {
            return;
        }
        if let DataDetector::Unresolved { int_range } = self {
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
            DataDetector::Unresolved { int_range } => {
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
                    .find(|(range, data_type)| range.contains(&min) && range.contains(&max))
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
                .map(|m| {
                    let mut data = DataDetector::new();
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
    let key_name = format!("{}Id", csv_name.to_upper_camel_case());
    let mut s = Struct::new(csv_name);
    let mut i = Impl::new(csv_name);
    apply_derives(&mut s).vis("pub");
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
                    .replace("{", "_")
                    .replace("}", "")
                    .replace("[", "_")
                    .replace("]", "")
                    .replace("PvP", "Pvp")
                    .to_snake_case()
            };
            if line_one == "type" {
                line_one = "r#type".to_string();
            } else if line_one == "trait" {
                line_one = "r#trait".to_string();
            } else if line_one == "move" {
                line_one = "r#move".to_string();
            } else if ('0'..='9').contains(&line_one.chars().next().unwrap_or_default()) {
                line_one = format!("num{line_one}");
            }

            lazy_static! {
                // regex: check is int type
                static ref INT: Regex = Regex::new(r#"^(u|)int(8|16|32|64)$"#).unwrap();
                // regex: check is bit offset
                static ref BIT: Regex = Regex::new(r#"^bit(&[0-9]+|)$"#).unwrap();
            }
            if BIT.is_match(field_value) {
                (line_one, "bool".to_string())
            } else if field_value == "sbyte" {
                // TODO add serde attribute for this
                (line_one, "bool".to_string())
            } else if INT.is_match(field_value) {
                let mut line_two = field_value.replace("int", "");
                // uint64 -> u64
                // int64 -> 64, add the i if no u
                if !line_two.starts_with("u") {
                    line_two = format!("i{}", line_two);
                }

                if line_one == "key_id" {
                    let mut key = Struct::new(&key_name);
                    apply_derives(&mut key).tuple_field(&line_two).vis("pub").derive("Hash").derive("Eq").derive("PartialEq").derive("Copy");
                    scope.push_struct(key);
                    let mut key_impl = Impl::new(&key_name);
                    key_impl.new_fn("new").arg("value", &line_two).line(format!("Self(value)")).ret("Self");
                    key_impl.new_fn("inner").arg_ref_self().line("self.0").ret(&line_two).vis("pub");
                    scope.push_impl(key_impl);
                    line_two = key_name.clone();
                    let db_field_name = format!("{}s", csv_name.to_snake_case());
                    let db_field_key = format!("HashMap<{key_name}, {csv_name}>");
                    args.db
                        .field(&db_field_name, &db_field_key);
                    args.db_impl.new_fn(&format!("set_{db_field_name}")).vis("pub").arg_mut_self().line(format!("self.{db_field_name} = arg;")).arg("arg", &db_field_key);
                    args.db_impl.new_fn(&format!("get_{db_field_name}")).vis("pub").arg_ref_self().line(format!("&self.{db_field_name}")).ret(&format!("&{db_field_key}"));
                    args.read_data.line(format!("data.set_{db_field_name}(read_csv::<{csv_name}>(r#\"{path}\"#).into_iter().map(|m| (m.get_key_id(), m)).collect());"));
                    local_data.known_structs.insert(key_name.clone());
                }
                (line_one, line_two)
            } else if field_value == "byte" {
                (line_one, "u8".to_string())
            } else if field_value == "str" {
                (line_one, "String".to_string())
            } else {
                // remove trailing numbers from the field_name before adding the ID
                let field_name = field_name.to_upper_camel_case();
                let mut field_name: String =
                    field_name.chars().filter(|c| !c.is_numeric()).collect();
                if field_name.is_empty() {
                    (line_one, "String".to_string())
                } else {
                    let local_key_name = format!("{}Id", field_value);
                    if !local_data
                        .requested_structs
                        .iter()
                        .any(|d| d.requested_struct == local_key_name)
                    {
                        local_data.requested_structs.push(RequestedStructData {
                            requested_struct: local_key_name.clone(),
                            sample_data: sample_data.to_string(),
                        });
                    }
                    (line_one, local_key_name)
                }
            }
        })
        .collect();
    for (field_name, field_value) in &fields {
        let mut function = Function::new(&format!("get_{}", field_name.replace("#", "")));
        function
            .vis("pub")
            .arg_ref_self()
            .line(format!("self.{field_name}.clone()"))
            .ret(field_value);
        i.push_fn(function);
        let mut field = Field::new(field_name, field_value);
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
        s.push_field(field);
    }
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
            } else if ft.is_file() {
                if let Some(ext) = file.path().extension() {
                    if ext == "csv" {
                        let file_name = file.file_name();
                        let mut file_name = file_name.to_str().unwrap().split(".");
                        let file_name = file_name.next().unwrap().to_string();
                        if let Some(filter) = &args.list_filter {
                            if !filter.contains(&file_name) {
                                continue;
                            }
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
             sample_data,
         }| !local_data.known_structs.contains(requested_struct),
    ) {
        let mut s = Struct::new(&requested_struct);
        apply_derives(&mut s).vis("pub").tuple_field(sample_data);
        scope.push_struct(s);
    }
    scope
}

fn main() {
    let mut args: Args = Args {
        recurse_directories: false,
        // bin_code_generation: false,
        bin_code_generation: false,
        list_filter: Some(vec![
            "Item".to_string(),
            "Recipe".to_string(),
            "RecipeLookup".to_string(),
        ]),
        db: Struct::new("Data"),
        db_impl: Impl::new("Data"),
        read_data: Function::new("read_data"),
    };

    // Start the read function with the data header
    args.read_data.line("let mut data = Data::default();");

    args.recurse_directories = false;
    let out_dir = env::var_os("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("types.rs");
    let dir = "G:/Code/ffxiv-datamining/csv/";
    let scope = Scope::new();
    let mut scope = read_dir(PathBuf::from_str(dir).unwrap(), scope, &mut args);
    apply_derives(&mut args.db).vis("pub").derive("Default");
    scope.push_struct(args.db);
    scope.push_impl(args.db_impl);
    scope.import("std::collections", "HashMap");
    write(dest_path, scope.to_string()).unwrap();

    let conversion_files = Path::new(&out_dir).join("serialization.rs");

    args.read_data.line("data").ret("Data");

    let mut ser_scope = Scope::new();
    ser_scope.import("xiv_gen", "*");
    ser_scope.push_fn(args.read_data);
    write(conversion_files, ser_scope.to_string()).unwrap();

    println!("cargo:rerun-if-changed=build.rs");
}
