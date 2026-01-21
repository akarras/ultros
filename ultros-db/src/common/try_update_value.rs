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
