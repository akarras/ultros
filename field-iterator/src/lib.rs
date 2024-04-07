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
}
