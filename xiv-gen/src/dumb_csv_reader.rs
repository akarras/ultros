use dumb_csv::DumbCsvDeserialize;

pub fn read_dumb_csv<T: DumbCsvDeserialize>(path: &str) -> Vec<T> {
    let mut csv = csv::ReaderBuilder::new()
        .has_headers(false)
        .from_path(path)
        .expect("Failed to open csv");
    let headers: Vec<String> = csv
        .records()
        .nth(1)
        .unwrap()
        .unwrap()
        .iter()
        .map(|s| s.to_string())
        .collect();
    dumb_csv::deserialize(csv).unwrap()
    
}

#[cfg(test)]
fn test_reader() {}
