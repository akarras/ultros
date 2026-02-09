#![allow(unused_imports)]
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
    // Skip header
    let _ = csv.records().next();
    dumb_csv::deserialize(csv).unwrap()
}

pub fn read_csv<T: DeserializeOwned>(path: &str) -> Vec<T> {
    let mut csv = csv::ReaderBuilder::new()
        .has_headers(false)
        .from_path(path)
        .expect("Failed to open csv");

    // Get headers for error reporting (and skip them)
    let headers_record = csv.records().next().unwrap().unwrap();
    let headers: Vec<String> = headers_record.iter().map(|s| s.to_string()).collect();

    csv.deserialize()
        .map(|m| {
            if let Err(e) = &m {
                // try to pretty print this error a bit, otherwise it's hard to tell what went wrong
                if e.position().is_some() {
                    if let ErrorKind::Deserialize { err, .. } = e.kind()
                        && let Some(field) = err.field()
                        && let Some(field_name) = headers.get(field as usize)
                    {
                        eprintln!("Field {field}: {field_name}");
                    }
                    eprintln!("{e:?} error in {path}");
                }
            }
            m.unwrap_or_else(|_| panic!("Failed to deserialize file {}", path))
        })
        .collect()
}
