use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Default, Debug, PartialEq, PartialOrd, Eq, Ord, Clone)]
pub struct Retainer {
    pub id: i32,
    pub world_id: i32,
    pub name: String,
    pub retainer_city_id: i32,
}
