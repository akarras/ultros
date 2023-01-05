use crate::world::{Datacenter, Region, World, WorldData};
use serde::{Deserialize, Serialize};

/// Like world_cache but built for use in wasm
#[derive(Serialize, Deserialize)]
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

    pub fn lookup_selector<'a>(&'a self, selector: AnySelector) -> Option<AnyResult<'a>> {
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
}
