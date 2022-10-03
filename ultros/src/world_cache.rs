use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use ultros_db::{
    entity::{datacenter, region, world},
    UltrosDb,
};

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub enum AnySelector {
    World(i32),
    Datacenter(i32),
    Region(i32),
}

#[derive(Debug, Serialize)]
pub enum AnyResult<'a> {
    World(&'a world::Model),
    Datacenter(&'a datacenter::Model),
    Region(&'a region::Model),
}

impl<'a> AnyResult<'a> {
    pub(crate) fn get_name(&self) -> &str {
        match self {
            AnyResult::World(world) => &world.name,
            AnyResult::Datacenter(datacenter) => &datacenter.name,
            AnyResult::Region(region) => &region.name,
        }
    }
}

pub struct WorldCache {
    worlds: HashMap<i32, world::Model>,
    datacenter: HashMap<i32, datacenter::Model>,
    regions: HashMap<i32, region::Model>,
    datacenter_to_world: HashMap<i32, Vec<i32>>,
    region_to_worlds: HashMap<i32, Vec<i32>>,
    name_map: HashMap<String, AnySelector>,
}

impl WorldCache {
    pub async fn new(db: &UltrosDb) -> Self {
        let (worlds, datacenters, regions) = db
            .get_all_worlds_regions_and_datacenters()
            .await
            .expect("World query shouldn't ever fail");
        let name_map: HashMap<_, _> = worlds
            .iter()
            .map(|i| (i.name.clone(), AnySelector::World(i.id)))
            .chain(
                datacenters
                    .iter()
                    .map(|i| (i.name.clone(), AnySelector::Datacenter(i.id))),
            )
            .chain(
                regions
                    .iter()
                    .map(|i| (i.name.clone(), AnySelector::Region(i.id))),
            )
            .collect();

        let datacenter_to_world =
            worlds
                .iter()
                .fold(HashMap::<i32, Vec<i32>>::new(), |mut map, world| {
                    map.entry(world.datacenter_id).or_default().push(world.id);
                    map
                });
        let region_to_worlds = datacenter_to_world.iter().fold(
            HashMap::<i32, Vec<i32>>::new(),
            |mut map, (datacenter, worlds)| {
                let datacenter = datacenters.iter().find(|d| d.id == *datacenter).unwrap();
                map.entry(datacenter.region_id).or_default().extend(worlds);
                map
            },
        );
        Self {
            worlds: worlds.into_iter().fold(HashMap::new(), |mut map, world| {
                map.insert(world.id, world);
                map
            }),
            datacenter: datacenters
                .into_iter()
                .fold(HashMap::new(), |mut map, datacenter| {
                    map.insert(datacenter.id, datacenter);
                    map
                }),
            regions: regions.into_iter().fold(HashMap::new(), |mut map, region| {
                map.insert(region.id, region);
                map
            }),
            name_map,
            datacenter_to_world,
            region_to_worlds,
        }
    }

    pub fn lookup_selector(&self, selector: &AnySelector) -> Option<AnyResult> {
        match selector {
            AnySelector::World(world) => Some(AnyResult::World(self.worlds.get(world)?)),
            AnySelector::Datacenter(datacenter) => {
                Some(AnyResult::Datacenter(self.datacenter.get(datacenter)?))
            }
            AnySelector::Region(region) => Some(AnyResult::Region(self.regions.get(region)?)),
        }
    }

    pub fn lookup_value_by_name(&self, name: &str) -> Option<AnyResult> {
        self.name_map
            .get(name)
            .map(|selector| self.lookup_selector(selector))
            .flatten()
    }

    pub fn get_all_worlds_in(&self, result: &AnyResult) -> Option<Vec<i32>> {
        match result {
            AnyResult::World(world) => Some(vec![world.id]),
            AnyResult::Datacenter(datacenter) => self
                .datacenter_to_world
                .get(&datacenter.id)
                .map(|i| i.clone()),
            AnyResult::Region(region) => self.region_to_worlds.get(&region.id).map(|i| i.clone()),
        }
    }

    pub fn get_datacenter(&self, result: &AnyResult) -> Option<&datacenter::Model> {
        match result {
            AnyResult::World(world) => self.datacenter.get(&world.datacenter_id),
            AnyResult::Datacenter(datacenter) => self.datacenter.get(&datacenter.id),
            AnyResult::Region(region) => self
                .datacenter
                .values()
                .find(|datacenter| datacenter.region_id == region.id),
        }
    }

    pub fn get_region(&self, result: &AnyResult) -> Option<&region::Model> {
        match result {
            AnyResult::World(world) => {
                let datacenter = self.datacenter.get(&world.datacenter_id)?;
                self.regions.get(&datacenter.region_id)
            }
            AnyResult::Datacenter(datacenter) => self.regions.get(&datacenter.region_id),
            AnyResult::Region(region) => self.regions.get(&region.id),
        }
    }
}
