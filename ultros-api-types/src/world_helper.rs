use crate::world::{Datacenter, Region, World, WorldData};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;

/// Like world_cache but built for use in wasm
#[derive(Serialize, Clone)]
pub struct WorldHelper {
    world_data: WorldData,
    #[serde(skip)]
    region_lookup: HashMap<i32, usize>,
    #[serde(skip)]
    dc_lookup: HashMap<i32, (usize, usize)>,
    #[serde(skip)]
    world_lookup: HashMap<i32, (usize, usize, usize)>,
}

impl<'de> Deserialize<'de> for WorldHelper {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Helper {
            world_data: WorldData,
        }
        let helper = Helper::deserialize(deserializer)?;
        Ok(WorldHelper::from(helper.world_data))
    }
}

impl From<WorldData> for WorldHelper {
    fn from(world_data: WorldData) -> Self {
        let (region_lookup, dc_lookup, world_lookup) = Self::build_indices(&world_data);
        Self {
            world_data,
            region_lookup,
            dc_lookup,
            world_lookup,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub enum AnyResult<'a> {
    Region(&'a Region),
    Datacenter(&'a Datacenter),
    World(&'a World),
}

impl<'a> From<AnyResult<'a>> for OwnedResult {
    fn from(value: AnyResult<'a>) -> Self {
        match value {
            AnyResult::Region(r) => Self::Region(r.clone()),
            AnyResult::Datacenter(d) => Self::Datacenter(d.clone()),
            AnyResult::World(w) => Self::World(w.clone()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub enum OwnedResult {
    Region(Region),
    Datacenter(Datacenter),
    World(World),
}

impl OwnedResult {
    pub fn get_name(&self) -> &str {
        match self {
            OwnedResult::Region(r) => &r.name,
            OwnedResult::Datacenter(d) => &d.name,
            OwnedResult::World(w) => &w.name,
        }
    }

    pub fn as_ref(&self) -> AnyResult<'_> {
        match self {
            OwnedResult::Region(r) => AnyResult::Region(r),
            OwnedResult::Datacenter(d) => AnyResult::Datacenter(d),
            OwnedResult::World(w) => AnyResult::World(w),
        }
    }
}

impl From<OwnedResult> for AnySelector {
    fn from(value: OwnedResult) -> Self {
        match value {
            OwnedResult::Region(r) => AnySelector::Region(r.id),
            OwnedResult::Datacenter(d) => AnySelector::Datacenter(d.id),
            OwnedResult::World(w) => AnySelector::World(w.id),
        }
    }
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AnySelector {
    Region(i32),
    Datacenter(i32),
    World(i32),
}

impl AnySelector {
    pub fn as_world_id(&self) -> Option<i32> {
        match self {
            Self::World(id) => Some(*id),
            _ => None,
        }
    }
}

impl<'a> From<&AnyResult<'a>> for AnySelector {
    fn from(value: &AnyResult<'a>) -> Self {
        match value {
            AnyResult::Region(r) => AnySelector::Region(r.id),
            AnyResult::Datacenter(d) => AnySelector::Datacenter(d.id),
            AnyResult::World(w) => AnySelector::World(w.id),
        }
    }
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

    /// Determines whether this result is inside of the other result
    /// for example, if this is the Datacenter Aether, and other is North-America,
    /// then this would be true.
    /// If it is the same object, it will also be true.
    pub fn is_in(&self, other: &Self) -> bool {
        match (self, other) {
            (AnyResult::Region(r1), AnyResult::Region(r2)) => r1.id == r2.id,
            (AnyResult::Datacenter(d1), AnyResult::Datacenter(d2)) => d1.id == d2.id,
            (AnyResult::Datacenter(d1), AnyResult::Region(r1)) => d1.region_id == r1.id,
            (AnyResult::World(w1), AnyResult::Region(r1)) => {
                r1.datacenters.iter().any(|d| d.id == w1.datacenter_id)
            }
            (AnyResult::World(w1), AnyResult::Datacenter(d1)) => w1.datacenter_id == d1.id,
            (AnyResult::World(w1), AnyResult::World(w2)) => w1.id == w2.id,
            _ => false,
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

impl<'a> From<&'a World> for AnyResult<'a> {
    fn from(value: &'a World) -> Self {
        AnyResult::World(value)
    }
}

impl<'a> From<&'a Datacenter> for AnyResult<'a> {
    fn from(value: &'a Datacenter) -> Self {
        AnyResult::Datacenter(value)
    }
}

impl<'a> From<&'a Region> for AnyResult<'a> {
    fn from(value: &'a Region) -> Self {
        AnyResult::Region(value)
    }
}

impl WorldHelper {
    #[allow(clippy::type_complexity)]
    fn build_indices(
        world_data: &WorldData,
    ) -> (
        HashMap<i32, usize>,
        HashMap<i32, (usize, usize)>,
        HashMap<i32, (usize, usize, usize)>,
    ) {
        let mut region_lookup = HashMap::new();
        let mut dc_lookup = HashMap::new();
        let mut world_lookup = HashMap::new();

        for (r_idx, region) in world_data.regions.iter().enumerate() {
            region_lookup.insert(region.id, r_idx);
            for (d_idx, datacenter) in region.datacenters.iter().enumerate() {
                dc_lookup.insert(datacenter.id, (r_idx, d_idx));
                for (w_idx, world) in datacenter.worlds.iter().enumerate() {
                    world_lookup.insert(world.id, (r_idx, d_idx, w_idx));
                }
            }
        }
        (region_lookup, dc_lookup, world_lookup)
    }

    pub fn new(world_data: WorldData) -> Self {
        let (region_lookup, dc_lookup, world_lookup) = Self::build_indices(&world_data);
        Self {
            world_data,
            region_lookup,
            dc_lookup,
            world_lookup,
        }
    }

    /// Ignores case and looks up the world name
    pub fn lookup_world_by_name(&self, name: &str) -> Option<AnyResult<'_>> {
        let mut worlds = self.world_data.regions.iter().flat_map(|region| {
            [AnyResult::Region(region)]
                .into_iter()
                .chain(region.datacenters.iter().flat_map(|dc| {
                    [AnyResult::Datacenter(dc)]
                        .into_iter()
                        .chain(dc.worlds.iter().map(AnyResult::World))
                }))
        });
        worlds.find(|any| any.get_name().eq_ignore_ascii_case(name))
    }
}

impl<'a> WorldHelper {
    pub fn lookup_selector(&'a self, selector: AnySelector) -> Option<AnyResult<'a>> {
        match selector {
            AnySelector::Region(r) => {
                let idx = self.region_lookup.get(&r)?;
                Some(AnyResult::Region(&self.world_data.regions[*idx]))
            }
            AnySelector::Datacenter(dc) => {
                let (r_idx, d_idx) = self.dc_lookup.get(&dc)?;
                Some(AnyResult::Datacenter(
                    &self.world_data.regions[*r_idx].datacenters[*d_idx],
                ))
            }
            AnySelector::World(w) => {
                let (r_idx, d_idx, w_idx) = self.world_lookup.get(&w)?;
                Some(AnyResult::World(
                    &self.world_data.regions[*r_idx].datacenters[*d_idx].worlds[*w_idx],
                ))
            }
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

    pub fn iter(&'a self) -> impl Iterator<Item = AnyResult<'a>> {
        self.world_data.regions.iter().flat_map(|r| {
            [AnyResult::Region(r)]
                .into_iter()
                .chain(r.datacenters.iter().flat_map(|d| {
                    [AnyResult::Datacenter(d)]
                        .into_iter()
                        .chain(d.worlds.iter().map(AnyResult::World))
                }))
        })
    }

    pub fn get_inner_data(&'a self) -> &'a WorldData {
        &self.world_data
    }

    pub fn get_cloned(&self) -> WorldData {
        self.world_data.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{Datacenter, Region, World};

    #[test]
    fn test_lookup_selector() {
        let world = World {
            id: 101,
            name: "TestWorld".to_string(),
            datacenter_id: 11,
        };
        let datacenter = Datacenter {
            id: 11,
            name: "TestDC".to_string(),
            region_id: 1,
            worlds: vec![world],
        };
        let region = Region {
            id: 1,
            name: "TestRegion".to_string(),
            datacenters: vec![datacenter],
        };
        let world_data = WorldData {
            regions: vec![region],
        };

        let helper = WorldHelper::new(world_data);

        // Test World Lookup
        let result = helper.lookup_selector(AnySelector::World(101));
        assert!(result.is_some());
        if let Some(AnyResult::World(w)) = result {
            assert_eq!(w.id, 101);
            assert_eq!(w.name, "TestWorld");
        } else {
            panic!("Expected World result");
        }

        // Test Datacenter Lookup
        let result = helper.lookup_selector(AnySelector::Datacenter(11));
        assert!(result.is_some());
        if let Some(AnyResult::Datacenter(d)) = result {
            assert_eq!(d.id, 11);
            assert_eq!(d.name, "TestDC");
        } else {
            panic!("Expected Datacenter result");
        }

        // Test Region Lookup
        let result = helper.lookup_selector(AnySelector::Region(1));
        assert!(result.is_some());
        if let Some(AnyResult::Region(r)) = result {
            assert_eq!(r.id, 1);
            assert_eq!(r.name, "TestRegion");
        } else {
            panic!("Expected Region result");
        }

        // Test Non-existent
        assert!(helper.lookup_selector(AnySelector::World(999)).is_none());
    }
}
