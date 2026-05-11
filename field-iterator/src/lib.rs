pub use field_iterator_macros::*;

pub trait FieldLabels {
    fn field_labels() -> &'static [&'static str];
}

pub trait SortableVec
where
    Self: Sized,
{
    fn sort_vec_by_label(vec: &mut Vec<Self>, field_label: &str, then_by: Option<&str>);
}

#[cfg(test)]
mod test {
    use super::SortableVec;

    #[allow(dead_code)]
    struct SomeType {
        value: i32,
        other_value: String,
    }

    impl SortableVec for SomeType {
        fn sort_vec_by_label(vec: &mut Vec<Self>, field_label: &str, then_by: Option<&str>) {
            vec.sort_by(move |a, b| {
                let mut cmp = match field_label {
                    "value" => a.value.cmp(&b.value),
                    "other_value" => a.other_value.cmp(&b.other_value),
                    _ => panic!("Invalid index"),
                };
                if let Some(then_by) = then_by {
                    cmp = cmp.then_with(|| match then_by {
                        "value" => a.value.cmp(&b.value),
                        "other_value" => a.other_value.cmp(&b.other_value),
                        _ => panic!("Invalid index"),
                    });
                }
                cmp
            });
        }
    }

    fn sample() -> Vec<SomeType> {
        vec![
            SomeType {
                value: 3,
                other_value: "b".into(),
            },
            SomeType {
                value: 1,
                other_value: "c".into(),
            },
            SomeType {
                value: 2,
                other_value: "a".into(),
            },
        ]
    }

    #[test]
    fn sort_by_value_ascending() {
        let mut v = sample();
        SomeType::sort_vec_by_label(&mut v, "value", None);
        assert_eq!(v.iter().map(|x| x.value).collect::<Vec<_>>(), vec![1, 2, 3]);
    }

    #[test]
    fn sort_by_other_value_lexical() {
        let mut v = sample();
        SomeType::sort_vec_by_label(&mut v, "other_value", None);
        let names: Vec<_> = v.iter().map(|x| x.other_value.clone()).collect();
        assert_eq!(names, vec!["a", "b", "c"]);
    }

    #[test]
    fn sort_by_primary_then_secondary_breaks_ties() {
        let mut v = vec![
            SomeType {
                value: 1,
                other_value: "z".into(),
            },
            SomeType {
                value: 1,
                other_value: "a".into(),
            },
            SomeType {
                value: 1,
                other_value: "m".into(),
            },
        ];
        SomeType::sort_vec_by_label(&mut v, "value", Some("other_value"));
        let names: Vec<_> = v.iter().map(|x| x.other_value.clone()).collect();
        assert_eq!(names, vec!["a", "m", "z"]);
    }

    #[test]
    #[should_panic(expected = "Invalid index")]
    fn unknown_field_label_panics() {
        let mut v = sample();
        SomeType::sort_vec_by_label(&mut v, "nope", None);
    }

    #[test]
    #[should_panic(expected = "Invalid index")]
    fn unknown_then_by_field_panics_when_comparing_tied_keys() {
        // The closure only evaluates the secondary key on equal primary, so we need a tie.
        let mut v = vec![
            SomeType {
                value: 1,
                other_value: "x".into(),
            },
            SomeType {
                value: 1,
                other_value: "y".into(),
            },
        ];
        SomeType::sort_vec_by_label(&mut v, "value", Some("nope"));
    }
}
