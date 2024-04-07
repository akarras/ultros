pub use dumb_csv_macros::DumbCsvDeserialize;

pub trait FromBool {
    fn from_bool(&self) -> bool;
}

impl FromBool for &str {
    fn from_bool(&self) -> bool {
        match *self {
            "TRUE" => true,
            "FALSE" => false,
            "1" => true,
            "0" => false,
            "True" => true,
            "False" => false,
            p => panic!("Unknown value {p}"),
        }
    }
}

pub fn deserialize<T, R>(mut rdr: csv::Reader<R>) -> Result<Vec<T>, csv::Error>
where
    T: DumbCsvDeserialize,
    R: std::io::Read,
{
    let mut data = vec![];

    for record in rdr.records() {
        let record = record?;

        data.push(T::from_str_list(record.iter()));
    }
    Ok(data)
}

pub trait DumbCsvDeserialize {
    fn from_str_list<'a>(csv: impl Iterator<Item = &'a str>) -> Self;
}

pub fn add(left: usize, right: usize) -> usize {
    left + right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(PartialEq, Debug)]
    struct SomeType {
        a: u32,
        b: Vec<u32>,
        c: Vec<u32>,
    }

    impl DumbCsvDeserialize for SomeType {
        fn from_str_list<'a>(mut csv: impl Iterator<Item = &'a str>) -> Self {
            Self {
                a: csv.next().map(|p| p.parse().unwrap()).unwrap(),
                b: csv.by_ref().take(2).map(|p| p.parse().unwrap()).collect(),
                c: csv.by_ref().take(2).map(|p| p.parse().unwrap()).collect(),
            }
        }
    }

    #[test]
    fn test_from_list() {
        let parts = ["235", "230949", "294920", "10949", "40202"].into_iter();
        assert_eq!(
            SomeType::from_str_list(parts),
            SomeType {
                a: 235,
                b: vec![230949, 294920],
                c: vec![10949, 40202]
            }
        );
    }
}
