use std::{borrow::Borrow, collections::HashMap, mem};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use ultros_db::{
    entity::{datacenter, region, world},
    UltrosDb,
};
use yoke::{Yoke, Yokeable};

pub type AllWorldsAndRegions<'a> = Vec<(
    &'a region::Model,
    Vec<(&'a datacenter::Model, Vec<&'a world::Model>)>,
)>;

#[derive(Debug, Serialize, Deserialize, Clone, Copy, Eq, PartialEq, PartialOrd, Ord, Hash)]
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

impl<'a> From<&'a AnyResult<'a>> for AnySelector {
    fn from(result: &AnyResult) -> Self {
        match result {
            AnyResult::World(world) => Self::World(world.id),
            AnyResult::Datacenter(dc) => Self::Datacenter(dc.id),
            AnyResult::Region(region) => Self::Region(region.id),
        }
    }
}

impl<'a> AnyResult<'a> {
    pub fn as_world(&self) -> Result<&'a world::Model, WorldCacheError> {
        match self {
            AnyResult::World(w) => Ok(w),
            _ => Err(WorldCacheError::NotWorld),
        }
    }
}

#[derive(Debug, Error)]
pub enum WorldCacheError {
    #[error("Failed to get world by id {0}")]
    World(i32),
    #[error("Failed to get datacenter by id {0}")]
    Datacenter(i32),
    #[error("Failed to get region by id {0}")]
    Region(i32),
    #[error("Name lookup error {0}")]
    NameLookupError(String),
    #[error("Not a world")]
    NotWorld,
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

#[derive(Debug)]
struct RawData {
    worlds: HashMap<i32, world::Model>,
    datacenters: HashMap<i32, datacenter::Model>,
    regions: HashMap<i32, region::Model>,
}

pub struct WorldCache {
    yoke: Yoke<VirtualData<'static>, Box<RawData>>,
    datacenter_to_world: HashMap<i32, Vec<i32>>,
    region_to_worlds: HashMap<i32, Vec<i32>>,
    name_map: HashMap<String, AnySelector>,
}

impl std::fmt::Debug for WorldCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorldCache")
            .field("datacenter_to_world", &self.datacenter_to_world)
            .field("region_to_worlds", &self.region_to_worlds)
            .field("name_map", &self.name_map)
            .finish()
    }
}

#[derive(Debug)]
struct VirtualData<'a> {
    /// Represents a Vec with a list to all worlds
    all: AllWorldsAndRegions<'a>,
}

unsafe impl<'a> Yokeable<'a> for VirtualData<'static> {
    type Output = VirtualData<'a>;
    #[inline]
    fn transform(&'a self) -> &'a VirtualData<'a> {
        self
    }
    #[inline]
    fn transform_owned(self) -> VirtualData<'a> {
        self
    }
    #[inline]
    unsafe fn make(from: VirtualData<'a>) -> Self {
        let ret = mem::transmute_copy(&from);
        mem::forget(from);
        ret
    }
    #[inline]
    fn transform_mut<F>(&'a mut self, f: F)
    where
        F: 'static + FnOnce(&'a mut Self::Output),
    {
        unsafe { f(mem::transmute(self)) }
    }
}

fn yoke(raw_data: Box<RawData>) -> Yoke<VirtualData<'static>, Box<RawData>> {
    Yoke::<VirtualData<'static>, Box<RawData>>::attach_to_cart(raw_data, |r: &RawData| {
        VirtualData::new(r)
    })
}

impl<'a> VirtualData<'a> {
    fn new(raw_data: &'a RawData) -> Self {
        let mut temp = Self {
            all: raw_data
                .regions
                .values()
                .map(|r| {
                    (
                        r,
                        raw_data
                            .datacenters
                            .values()
                            .filter(|d| d.region_id == r.id)
                            .map(|d| {
                                (
                                    d,
                                    raw_data
                                        .worlds
                                        .values()
                                        .filter(|w| w.datacenter_id == d.id)
                                        .collect(),
                                )
                            })
                            .collect(),
                    )
                })
                .collect(),
        };
        temp.all.sort_by_key(|a| &a.0.name);
        for (_, datacenters) in &mut temp.all {
            datacenters.sort_by_key(|(d, _)| &d.name);
            for (_, worlds) in datacenters {
                worlds.sort_by_key(|w| &w.name);
            }
        }
        temp
    }
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
        let raw_data = RawData {
            worlds: worlds.into_iter().fold(HashMap::new(), |mut map, world| {
                map.insert(world.id, world);
                map
            }),
            datacenters: datacenters
                .into_iter()
                .fold(HashMap::new(), |mut map, datacenter| {
                    map.insert(datacenter.id, datacenter);
                    map
                }),
            regions: regions.into_iter().fold(HashMap::new(), |mut map, region| {
                map.insert(region.id, region);
                map
            }),
        };
        let yoke = yoke(Box::new(raw_data));
        Self {
            yoke,
            name_map,
            datacenter_to_world,
            region_to_worlds,
        }
    }

    pub fn lookup_selector(&self, selector: &AnySelector) -> Result<AnyResult, WorldCacheError> {
        let cart = self.yoke.backing_cart();
        let RawData {
            worlds,
            datacenters: datacenter,
            regions,
        } = cart.borrow();
        match selector {
            AnySelector::World(world) => {
                let world = worlds.get(world).ok_or(WorldCacheError::World(*world))?;
                Ok(AnyResult::World(world))
            }
            AnySelector::Datacenter(dc) => {
                let datacenter = datacenter.get(dc).ok_or(WorldCacheError::Datacenter(*dc))?;
                Ok(AnyResult::Datacenter(datacenter))
            }
            AnySelector::Region(region) => Ok(AnyResult::Region(
                regions
                    .get(region)
                    .ok_or(WorldCacheError::Region(*region))?,
            )),
        }
    }

    pub fn lookup_value_by_name(&self, name: &str) -> Result<AnyResult, WorldCacheError> {
        self.name_map
            .get(name)
            .and_then(|selector| self.lookup_selector(selector).ok())
            .ok_or_else(|| WorldCacheError::NameLookupError(name.to_string()))
    }

    pub fn get_all_worlds_in(&self, result: &AnyResult) -> Option<Vec<i32>> {
        match result {
            AnyResult::World(world) => Some(vec![world.id]),
            AnyResult::Datacenter(datacenter) => {
                self.datacenter_to_world.get(&datacenter.id).cloned()
            }
            AnyResult::Region(region) => self.region_to_worlds.get(&region.id).cloned(),
        }
    }

    pub fn get_datacenters(&self, result: &AnyResult) -> Option<Vec<&datacenter::Model>> {
        let cart = self.yoke.backing_cart();
        let RawData {
            datacenters: datacenter,
            ..
        } = cart.borrow();
        match result {
            AnyResult::World(world) => datacenter.get(&world.datacenter_id).map(|i| vec![i]),
            AnyResult::Datacenter(dc) => datacenter.get(&dc.id).map(|d| vec![d]),
            AnyResult::Region(region) => Some(
                datacenter
                    .values()
                    .filter(|datacenter| datacenter.region_id == region.id)
                    .collect(),
            ),
        }
    }

    pub fn get_region(&self, result: &AnyResult) -> Option<&region::Model> {
        let cart = self.yoke.backing_cart();
        let RawData {
            datacenters: datacenter,
            regions,
            ..
        } = cart.borrow();
        match result {
            AnyResult::World(world) => {
                let datacenter = datacenter.get(&world.datacenter_id)?;
                regions.get(&datacenter.region_id)
            }
            AnyResult::Datacenter(dc) => regions.get(&dc.region_id),
            AnyResult::Region(region) => regions.get(&region.id),
        }
    }

    pub fn get_all_regions(&self) -> Vec<&region::Model> {
        let cart = self.yoke.backing_cart();
        let RawData { regions, .. } = cart.borrow();
        regions.values().collect()
    }

    pub fn get_all(&self) -> &AllWorldsAndRegions {
        &self.yoke.get().all
    }
}
