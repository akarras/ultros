use std::str::FromStr;

pub use dumb_csv_macros::DumbCsvDeserialize;

pub trait ParseBool {
    fn parse_bool(&self) -> bool;
}

impl ParseBool for &str {
    fn parse_bool(&self) -> bool {
        bool_from_str(self).unwrap_or_default()
    }
}

pub fn bool_from_str(val: &str) -> Option<bool> {
    Some(match val {
        "TRUE" => true,
        "FALSE" => false,
        "1" => true,
        "0" => false,
        "True" => true,
        "False" => false,
        _ => return None,
    })
}

pub trait ParseOrDefault {
    fn parse_or_default<T>(str_val: &str) -> T
    where
        T: Default + FromStr;
}

impl ParseOrDefault for &str {
    fn parse_or_default<T>(str_val: &str) -> T
    where
        T: Default + FromStr,
    {
        str_val.parse().unwrap_or_default()
    }
}

pub fn deserialize<T, R>(mut rdr: csv::Reader<R>) -> Result<Vec<T>, csv::Error>
where
    T: DumbCsvDeserialize,
    R: std::io::Read,
{
    let mut data = vec![];
    let mut record = csv::StringRecord::new();

    while rdr.read_record(&mut record)? {
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

    #[test]
    fn test_deserialize() {
        let csv_data = "0,1,2,3,4\n5,6,7,8,9\n";

        let reader = csv::ReaderBuilder::new()
            .has_headers(false)
            .from_reader(csv_data.as_bytes());

        let result: Vec<SomeType> = deserialize(reader).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].a, 0);
        assert_eq!(result[1].a, 5);
        assert_eq!(result[1].b, vec![6, 7]);
        assert_eq!(result[1].c, vec![8, 9]);
    }
}
