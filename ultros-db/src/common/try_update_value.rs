use sea_orm::ActiveValue;
use sea_query::Value;

pub(crate) trait ActiveValueCmpSet<T> {
    fn cmp_set_value(&mut self, value: T);
}

impl<T> ActiveValueCmpSet<T> for ActiveValue<T>
where
    T: PartialEq + Into<Value> + Copy,
    sea_orm::Value: From<T>,
{
    fn cmp_set_value(&mut self, value: T) {
        *self = match self {
            ActiveValue::Set(_) => ActiveValue::Set(value),
            ActiveValue::Unchanged(v) => {
                let v = *v;
                if v == value {
                    ActiveValue::Unchanged(v)
                } else {
                    ActiveValue::Set(value)
                }
            }
            ActiveValue::NotSet => ActiveValue::NotSet,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::ActiveValue;

    #[test]
    fn unchanged_stays_unchanged_when_new_value_equals_old() {
        let mut v: ActiveValue<i32> = ActiveValue::Unchanged(42);
        v.cmp_set_value(42);
        assert!(matches!(v, ActiveValue::Unchanged(42)));
    }

    #[test]
    fn unchanged_becomes_set_when_new_value_differs() {
        let mut v: ActiveValue<i32> = ActiveValue::Unchanged(42);
        v.cmp_set_value(99);
        assert!(matches!(v, ActiveValue::Set(99)));
    }

    #[test]
    fn set_remains_set_with_updated_value() {
        let mut v: ActiveValue<i32> = ActiveValue::Set(1);
        v.cmp_set_value(2);
        assert!(matches!(v, ActiveValue::Set(2)));
    }

    #[test]
    fn set_remains_set_with_same_value() {
        let mut v: ActiveValue<i32> = ActiveValue::Set(7);
        v.cmp_set_value(7);
        assert!(matches!(v, ActiveValue::Set(7)));
    }

    #[test]
    fn not_set_remains_not_set_after_cmp_set() {
        let mut v: ActiveValue<i32> = ActiveValue::NotSet;
        v.cmp_set_value(123);
        assert!(matches!(v, ActiveValue::NotSet));
    }
}
