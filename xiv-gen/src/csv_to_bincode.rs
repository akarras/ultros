/// Contains all the code needed to read a csv file and save it to a .bincode database
/// Recommended to just let xiv-gen-db handle this unless you need a different backing store.
use crate::*;
use csv::ErrorKind;
use serde::de::DeserializeOwned;

include!(concat!(env!("OUT_DIR"), "/deserialization.rs"));

pub fn read_dumb_csv<T: DumbCsvDeserialize>(path: &str) -> Vec<T> {
    let mut csv = csv::ReaderBuilder::new()
        .has_headers(false)
        .from_path(path)
        .expect("Failed to open csv");
    let _headers: Vec<String> = csv
        .records()
        .nth(1)
        .unwrap()
        .unwrap()
        .iter()
        .map(|s| s.to_string())
        .collect();
    let _ = csv.records().take(2).collect::<Vec<_>>();
    dumb_csv::deserialize(csv).unwrap()
}

pub fn read_csv<T: DeserializeOwned>(path: &str) -> Vec<T> {
    let mut csv = csv::ReaderBuilder::new()
        .has_headers(false)
        .from_path(path)
        .expect("Failed to open csv");
    let str = std::fs::read_to_string(path).unwrap();
    let headers: Vec<String> = csv
        .records()
        .nth(1)
        .unwrap()
        .unwrap()
        .iter()
        .map(|s| s.to_string())
        .collect();
    // line 2
    csv.deserialize()
        .skip(2)
        .map(|m| {
            if let Err(e) = &m {
                // try to pretty print this error a bit, otherwise it's hard to tell what went wrong
                if let Some(position) = e.position() {
                    if let ErrorKind::Deserialize { err, .. } = e.kind()
                        && let Some(field) = err.field()
                    {
                        let field_name = &headers[field as usize];
                        eprintln!("Field {field}: {field_name}");
                    }
                    let byte = position.byte() as usize;
                    let start_index = str[0..byte].rfind('\n').unwrap_or(0);
                    // let start_index = (byte - 10).clamp(0, str.len());
                    let end_index = str[byte..].find('\n').unwrap_or(str.len()) + byte;
                    // let end_index = (byte + 10).clamp(0, str.len());
                    let value = &str[start_index..=end_index];
                    let start_index = byte - start_index;
                    eprintln!(
                        "{e:?}error\nstring sample\n{value}\n{:>start_index$} {path}",
                        "^".to_string()
                    );
                }
            }
            m.unwrap_or_else(|_| panic!("Failed to deserialize file {}", path))
        })
        .collect()
}
