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
    fn bool_from_str_recognized_truthy_strings() {
        assert_eq!(bool_from_str("TRUE"), Some(true));
        assert_eq!(bool_from_str("True"), Some(true));
        assert_eq!(bool_from_str("1"), Some(true));
    }

    #[test]
    fn bool_from_str_recognized_falsy_strings() {
        assert_eq!(bool_from_str("FALSE"), Some(false));
        assert_eq!(bool_from_str("False"), Some(false));
        assert_eq!(bool_from_str("0"), Some(false));
    }

    #[test]
    fn bool_from_str_returns_none_for_unknown_values() {
        assert_eq!(bool_from_str(""), None);
        assert_eq!(bool_from_str("true"), None); // lowercase not handled
        assert_eq!(bool_from_str("false"), None);
        assert_eq!(bool_from_str("yes"), None);
        assert_eq!(bool_from_str("2"), None);
        assert_eq!(bool_from_str(" TRUE "), None); // no trimming
    }

    #[test]
    fn parse_bool_trait_defaults_to_false_for_unknown() {
        assert!("TRUE".parse_bool());
        assert!(!"FALSE".parse_bool());
        // unknown input becomes default (false)
        assert!(!"banana".parse_bool());
        assert!(!"".parse_bool());
    }

    #[test]
    fn deserialize_reads_records_via_dumb_csv_deserialize() {
        let input = "1,2,3,4,5\n6,7,8,9,10\n";
        let rdr = csv::ReaderBuilder::new()
            .has_headers(false)
            .from_reader(input.as_bytes());
        let rows: Vec<SomeType> = deserialize(rdr).unwrap();
        assert_eq!(
            rows,
            vec![
                SomeType {
                    a: 1,
                    b: vec![2, 3],
                    c: vec![4, 5]
                },
                SomeType {
                    a: 6,
                    b: vec![7, 8],
                    c: vec![9, 10]
                }
            ]
        );
    }

    #[test]
    fn parse_or_default_uses_default_for_invalid_input() {
        let v: i32 = <&str as ParseOrDefault>::parse_or_default("abc");
        assert_eq!(v, 0);
        let v: i32 = <&str as ParseOrDefault>::parse_or_default("42");
        assert_eq!(v, 42);
    }
}
