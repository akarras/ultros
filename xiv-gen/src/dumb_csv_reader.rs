use dumb_csv::DumbCsvDeserialize;

pub fn read_dumb_csv<T: DumbCsvDeserialize>(path: &str) -> Vec<T> {
    let mut csv = csv::ReaderBuilder::new()
        .has_headers(false)
        .from_path(path)
        .expect("Failed to open csv");
    // Skip the first two rows (metadata/headers)
    let mut record = csv::StringRecord::new();
    if !csv.read_record(&mut record).expect("Failed to read CSV") {
        panic!("File empty, expected headers");
    }
    if !csv.read_record(&mut record).expect("Failed to read CSV") {
        panic!("File too short, expected headers");
    }
    dumb_csv::deserialize(csv).unwrap()
    
}

#[cfg(test)]
fn test_reader() {}
