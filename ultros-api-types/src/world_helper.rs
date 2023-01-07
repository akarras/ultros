use crate::world::{Datacenter, Region, World, WorldData};
use serde::{Deserialize, Serialize};

/// Like world_cache but built for use in wasm
#[derive(Serialize, Deserialize, Clone)]
pub struct WorldHelper {
    world_data: WorldData,
}

#[derive(Copy, Clone, Debug)]
pub enum AnyResult<'a> {
    Region(&'a Region),
    Datacenter(&'a Datacenter),
    World(&'a World),
}

#[derive(Copy, Clone, Debug)]
pub enum AnySelector {
    Region(i32),
    Datacenter(i32),
    World(i32),
}

impl<'a> AnyResult<'a> {
    /// Creates an iterator over all worlds within this result
    pub fn all_worlds(&self) -> impl Iterator<Item = &'a World> {
        let iterator: Box<dyn Iterator<Item = &'a World>> = match self {
            AnyResult::Region(region) => {
                Box::new(region.datacenters.iter().flat_map(|dc| dc.worlds.iter()))
            }
            AnyResult::Datacenter(datacenter) => Box::new(datacenter.worlds.iter()),
            AnyResult::World(world) => Box::new([*world].into_iter()),
        };
        iterator
    }

    pub fn as_region(&'_ self) -> Option<&'a Region> {
        match self {
            AnyResult::Region(r) => Some(r),
            _ => None,
        }
    }

    pub fn as_datacenter(&'_ self) -> Option<&'a Datacenter> {
        match self {
            AnyResult::Datacenter(dc) => Some(dc),
            _ => None,
        }
    }

    pub fn as_world(&'_ self) -> Option<&'a World> {
        match self {
            AnyResult::World(w) => Some(w),
            _ => None,
        }
    }
}

impl<'a> AnyResult<'a> {
    pub fn get_name(&self) -> &str {
        match self {
            AnyResult::Region(r) => r.name.as_str(),
            AnyResult::Datacenter(d) => d.name.as_str(),
            AnyResult::World(w) => w.name.as_str(),
        }
    }
}

impl WorldHelper {
    pub fn new(world_data: WorldData) -> Self {
        Self { world_data }
    }

    /// Ignores case and looks up the world name
    pub fn lookup_world_by_name(&self, name: &str) -> Option<AnyResult> {
        let mut worlds = self.world_data.regions.iter().flat_map(|region| {
            [AnyResult::Region(region)]
                .into_iter()
                .chain(region.datacenters.iter().flat_map(|dc| {
                    [AnyResult::Datacenter(dc)]
                        .into_iter()
                        .chain(dc.worlds.iter().map(|world| AnyResult::World(world)))
                }))
        });
        worlds.find(|any| any.get_name().eq_ignore_ascii_case(name))
    }
}

impl<'a> WorldHelper {
    pub fn lookup_selector(&'a self, selector: AnySelector) -> Option<AnyResult<'a>> {
        match selector {
            AnySelector::Region(r) => self
                .world_data
                .regions
                .iter()
                .find(|region| region.id == r)
                .map(|r| AnyResult::Region(r)),
            AnySelector::Datacenter(dc) => self
                .world_data
                .regions
                .iter()
                .flat_map(|r| r.datacenters.iter())
                .find(|d| d.id == dc)
                .map(|d| AnyResult::Datacenter(d)),
            AnySelector::World(w) => self
                .world_data
                .regions
                .iter()
                .flat_map(|r| r.datacenters.iter().flat_map(|dc| dc.worlds.iter()))
                .find(|world| world.id == w)
                .map(|w| AnyResult::World(w)),
        }
    }

    /// Returns all datacenters associated with the result.
    /// For a world or a datacenter this will basically always be *one*
    pub fn get_datacenters(&'a self, any_result: &AnyResult<'a>) -> Vec<&'a Datacenter> {
        match any_result {
            AnyResult::Region(region) => region.datacenters.iter().collect(),
            AnyResult::Datacenter(datacenter) => vec![datacenter],
            AnyResult::World(world) => {
                let datacenter: AnyResult<'a> = self
                    .lookup_selector(AnySelector::Datacenter(world.datacenter_id))
                    .unwrap();
                let datacenter: &Datacenter = datacenter.as_datacenter().unwrap();
                vec![datacenter]
            }
        }
    }

    pub fn get_region(&'a self, any_result: AnyResult<'a>) -> &'a Region {
        match any_result {
            AnyResult::Region(region) => region,
            AnyResult::Datacenter(datacenter) => {
                let region = self
                    .lookup_selector(AnySelector::Region(datacenter.region_id))
                    .unwrap();
                region.as_region().unwrap()
            }
            AnyResult::World(world) => {
                let datacenter: AnyResult<'a> = self
                    .lookup_selector(AnySelector::Datacenter(world.datacenter_id))
                    .unwrap();
                let datacenter: &Datacenter = datacenter.as_datacenter().unwrap();
                let region: AnyResult<'_> = self
                    .lookup_selector(AnySelector::Region(datacenter.region_id))
                    .unwrap();
                region.as_region().unwrap()
            }
        }
    }
}
