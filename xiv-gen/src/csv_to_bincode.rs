/// Contains all the code needed to read a csv file and save it to a .bincode database
/// Recommended to just let xiv-gen-db handle this unless you need a different backing store.
use crate::*;
use csv::ErrorKind;
use serde::de::DeserializeOwned;

include!(concat!(env!("OUT_DIR"), "/deserialization.rs"));

pub fn read_dumb_csv<T: DumbCsvDeserialize>(path: &str) -> Vec<T> {
    // Detect format
    let mut csv = csv::ReaderBuilder::new()
        .has_headers(false)
        .from_path(path)
        .expect("Failed to open csv");
    let mut records = csv.records();
    let _row0 = records.next().unwrap().unwrap();
    let row1 = records.next().unwrap().unwrap();
    let is_schema_less = row1.iter().next().map(|s| s.parse::<f64>().is_ok()).unwrap_or(false);
    drop(records);

    // Re-open
    let mut csv = csv::ReaderBuilder::new()
        .has_headers(false)
        .from_path(path)
        .expect("Failed to open csv");

    if is_schema_less {
        // Skip 1 row (Header)
        // dumb_csv::deserialize consumes remaining.
        // We need to skip 1.
        let _ = csv.records().next();
        dumb_csv::deserialize(csv).unwrap()
    } else {
        // Skip 4 rows
        let _ = csv.records().nth(3); // 0,1,2,3 consumed.
        dumb_csv::deserialize(csv).unwrap()
    }
}

pub fn read_csv<T: DeserializeOwned>(path: &str) -> Vec<T> {
    // Detect format
    let mut csv = csv::ReaderBuilder::new()
        .has_headers(false)
        .from_path(path)
        .expect("Failed to open csv");
    let mut records = csv.records();
    let row0 = records.next().unwrap().unwrap();
    let row1 = records.next().unwrap().unwrap();
    let is_schema_less = row1.iter().next().map(|s| s.parse::<f64>().is_ok()).unwrap_or(false);

    let headers: Vec<String> = if is_schema_less {
        row0.iter().map(|s| s.to_string()).collect()
    } else {
        // Original logic used Row 1 (Types) as headers?
        // Let's preserve that behavior for schema-full.
        row1.iter().map(|s| s.to_string()).collect()
    };
    drop(records);

    let mut csv = csv::ReaderBuilder::new()
        .has_headers(false)
        .from_path(path)
        .expect("Failed to open csv");

    let str = std::fs::read_to_string(path).unwrap();

    let skip_count = if is_schema_less { 1 } else { 4 };

    csv.deserialize()
        .skip(skip_count)
        .map(|m| {
            if let Err(e) = &m {
                // try to pretty print this error a bit, otherwise it's hard to tell what went wrong
                if let Some(position) = e.position() {
                    if let ErrorKind::Deserialize { err, .. } = e.kind()
                        && let Some(field) = err.field()
                    {
                        let field_name = if (field as usize) < headers.len() {
                            &headers[field as usize]
                        } else {
                            "Unknown"
                        };
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
