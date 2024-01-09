use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WorldData {
    pub regions: Vec<Region>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub struct Region {
    pub id: i32,
    pub name: String,
    pub datacenters: Vec<Datacenter>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub struct Datacenter {
    pub id: i32,
    pub name: String,
    pub region_id: i32,
    pub worlds: Vec<World>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub struct World {
    pub id: i32,
    pub name: String,
    pub datacenter_id: i32,
}
