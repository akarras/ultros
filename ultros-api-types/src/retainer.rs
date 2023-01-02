use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Retainer {
    pub id: i32,
    pub world_id: i32,
    pub name: String,
    pub retainer_city_id: i32,
}
