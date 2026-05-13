use crate::world::{Datacenter, Region, World, WorldData};
use serde::{Deserialize, Serialize};

/// Like world_cache but built for use in wasm
#[derive(Serialize, Deserialize, Clone)]
pub struct WorldHelper {
    world_data: WorldData,
}

impl From<WorldData> for WorldHelper {
    fn from(world_data: WorldData) -> Self {
        Self { world_data }
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
    pub fn new(world_data: WorldData) -> Self {
        Self { world_data }
    }

    /// Borrowed view of the underlying world data. Used by the server when
    /// inlining the world data into the initial HTML response so the client
    /// can skip the `/api/v1/world_data` fetch on hydration.
    pub fn world_data(&self) -> &WorldData {
        &self.world_data
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
            AnySelector::Region(r) => self
                .world_data
                .regions
                .iter()
                .find(|region| region.id == r)
                .map(AnyResult::Region),
            AnySelector::Datacenter(dc) => self
                .world_data
                .regions
                .iter()
                .flat_map(|r| r.datacenters.iter())
                .find(|d| d.id == dc)
                .map(AnyResult::Datacenter),
            AnySelector::World(w) => self
                .world_data
                .regions
                .iter()
                .flat_map(|r| r.datacenters.iter().flat_map(|dc| dc.worlds.iter()))
                .find(|world| world.id == w)
                .map(AnyResult::World),
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
        self.iter_with_region_priority(None)
    }

    /// Iterate the full hierarchy with regions reordered so that `preferred_region`
    /// (matched by name) is yielded first. Sort is stable, so the remaining regions
    /// keep their original relative order. Passing `None` matches `iter()`.
    pub fn iter_with_region_priority(
        &'a self,
        preferred_region: Option<&str>,
    ) -> impl Iterator<Item = AnyResult<'a>> {
        self.regions_ordered(preferred_region)
            .into_iter()
            .flat_map(|r| {
                [AnyResult::Region(r)]
                    .into_iter()
                    .chain(r.datacenters.iter().flat_map(|d| {
                        [AnyResult::Datacenter(d)]
                            .into_iter()
                            .chain(d.worlds.iter().map(AnyResult::World))
                    }))
            })
    }

    /// Return all regions, optionally with `preferred_region` (by name) moved to
    /// the front. Sort is stable.
    pub fn regions_ordered(&'a self, preferred_region: Option<&str>) -> Vec<&'a Region> {
        let mut regions: Vec<&Region> = self.world_data.regions.iter().collect();
        if let Some(pref) = preferred_region {
            regions.sort_by_key(|r| if r.name == pref { 0u8 } else { 1u8 });
        }
        regions
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
    use crate::world::{Datacenter, Region, World, WorldData};

    fn sample_world_data() -> WorldData {
        // Region 1 "North-America" → DC 10 "Aether" → Worlds 100 "Adamantoise", 101 "Cactuar"
        // Region 1 "North-America" → DC 11 "Primal" → World 110 "Behemoth"
        // Region 2 "Japan" → DC 20 "Elemental" → World 200 "Aegis"
        WorldData {
            regions: vec![
                Region {
                    id: 1,
                    name: "North-America".into(),
                    datacenters: vec![
                        Datacenter {
                            id: 10,
                            name: "Aether".into(),
                            region_id: 1,
                            worlds: vec![
                                World {
                                    id: 100,
                                    name: "Adamantoise".into(),
                                    datacenter_id: 10,
                                },
                                World {
                                    id: 101,
                                    name: "Cactuar".into(),
                                    datacenter_id: 10,
                                },
                            ],
                        },
                        Datacenter {
                            id: 11,
                            name: "Primal".into(),
                            region_id: 1,
                            worlds: vec![World {
                                id: 110,
                                name: "Behemoth".into(),
                                datacenter_id: 11,
                            }],
                        },
                    ],
                },
                Region {
                    id: 2,
                    name: "Japan".into(),
                    datacenters: vec![Datacenter {
                        id: 20,
                        name: "Elemental".into(),
                        region_id: 2,
                        worlds: vec![World {
                            id: 200,
                            name: "Aegis".into(),
                            datacenter_id: 20,
                        }],
                    }],
                },
            ],
        }
    }

    #[test]
    fn any_selector_as_world_id_only_for_world_variant() {
        assert_eq!(AnySelector::World(7).as_world_id(), Some(7));
        assert_eq!(AnySelector::Datacenter(7).as_world_id(), None);
        assert_eq!(AnySelector::Region(7).as_world_id(), None);
    }

    #[test]
    fn any_selector_serde_roundtrip() {
        for s in [
            AnySelector::Region(1),
            AnySelector::Datacenter(10),
            AnySelector::World(100),
        ] {
            let j = serde_json::to_string(&s).unwrap();
            let back: AnySelector = serde_json::from_str(&j).unwrap();
            assert_eq!(s, back);
        }
    }

    #[test]
    fn lookup_selector_finds_each_kind() {
        let helper: WorldHelper = sample_world_data().into();
        assert!(matches!(
            helper.lookup_selector(AnySelector::Region(1)),
            Some(AnyResult::Region(r)) if r.id == 1
        ));
        assert!(matches!(
            helper.lookup_selector(AnySelector::Datacenter(10)),
            Some(AnyResult::Datacenter(d)) if d.id == 10
        ));
        assert!(matches!(
            helper.lookup_selector(AnySelector::World(100)),
            Some(AnyResult::World(w)) if w.id == 100
        ));
    }

    #[test]
    fn lookup_selector_returns_none_for_unknown_id() {
        let helper: WorldHelper = sample_world_data().into();
        assert!(helper.lookup_selector(AnySelector::World(99999)).is_none());
        assert!(
            helper
                .lookup_selector(AnySelector::Datacenter(99999))
                .is_none()
        );
        assert!(helper.lookup_selector(AnySelector::Region(99999)).is_none());
    }

    #[test]
    fn lookup_world_by_name_is_case_insensitive() {
        let helper: WorldHelper = sample_world_data().into();
        let r = helper.lookup_world_by_name("adamantoise").unwrap();
        assert_eq!(r.get_name(), "Adamantoise");
        let r = helper.lookup_world_by_name("AETHER").unwrap();
        assert_eq!(r.get_name(), "Aether");
    }

    #[test]
    fn lookup_world_by_name_finds_region_dc_or_world() {
        let helper: WorldHelper = sample_world_data().into();
        assert!(matches!(
            helper.lookup_world_by_name("North-America"),
            Some(AnyResult::Region(_))
        ));
        assert!(matches!(
            helper.lookup_world_by_name("Primal"),
            Some(AnyResult::Datacenter(_))
        ));
        assert!(matches!(
            helper.lookup_world_by_name("Behemoth"),
            Some(AnyResult::World(_))
        ));
    }

    #[test]
    fn lookup_world_by_name_missing_returns_none() {
        let helper: WorldHelper = sample_world_data().into();
        assert!(helper.lookup_world_by_name("Nowhere").is_none());
    }

    #[test]
    fn any_result_is_in_self_is_true() {
        let helper: WorldHelper = sample_world_data().into();
        let world = helper.lookup_selector(AnySelector::World(100)).unwrap();
        assert!(world.is_in(&world));
    }

    #[test]
    fn any_result_world_is_in_its_datacenter_and_region() {
        let helper: WorldHelper = sample_world_data().into();
        let world = helper.lookup_selector(AnySelector::World(100)).unwrap();
        let dc = helper.lookup_selector(AnySelector::Datacenter(10)).unwrap();
        let region = helper.lookup_selector(AnySelector::Region(1)).unwrap();
        assert!(world.is_in(&dc));
        assert!(world.is_in(&region));
    }

    #[test]
    fn any_result_world_not_in_foreign_dc_or_region() {
        let helper: WorldHelper = sample_world_data().into();
        let na_world = helper.lookup_selector(AnySelector::World(100)).unwrap();
        let jp_dc = helper.lookup_selector(AnySelector::Datacenter(20)).unwrap();
        let jp_region = helper.lookup_selector(AnySelector::Region(2)).unwrap();
        assert!(!na_world.is_in(&jp_dc));
        assert!(!na_world.is_in(&jp_region));
    }

    #[test]
    fn any_result_datacenter_in_region_match() {
        let helper: WorldHelper = sample_world_data().into();
        let dc = helper.lookup_selector(AnySelector::Datacenter(10)).unwrap();
        let region = helper.lookup_selector(AnySelector::Region(1)).unwrap();
        assert!(dc.is_in(&region));
        let other_region = helper.lookup_selector(AnySelector::Region(2)).unwrap();
        assert!(!dc.is_in(&other_region));
    }

    #[test]
    fn any_result_dc_never_contains_region_or_world_when_swapped() {
        // The `is_in` impl has explicit asymmetric cases; the unhandled directions return false.
        let helper: WorldHelper = sample_world_data().into();
        let region = helper.lookup_selector(AnySelector::Region(1)).unwrap();
        let dc = helper.lookup_selector(AnySelector::Datacenter(10)).unwrap();
        let world = helper.lookup_selector(AnySelector::World(100)).unwrap();
        // Region is_in DC / Region is_in World — explicitly false branch
        assert!(!region.is_in(&dc));
        assert!(!region.is_in(&world));
        assert!(!dc.is_in(&world));
    }

    #[test]
    fn all_worlds_for_region_yields_every_world_in_every_dc() {
        let helper: WorldHelper = sample_world_data().into();
        let region = helper.lookup_selector(AnySelector::Region(1)).unwrap();
        let ids: Vec<_> = region.all_worlds().map(|w| w.id).collect();
        assert_eq!(ids, vec![100, 101, 110]);
    }

    #[test]
    fn all_worlds_for_datacenter_yields_only_its_worlds() {
        let helper: WorldHelper = sample_world_data().into();
        let dc = helper.lookup_selector(AnySelector::Datacenter(10)).unwrap();
        let ids: Vec<_> = dc.all_worlds().map(|w| w.id).collect();
        assert_eq!(ids, vec![100, 101]);
    }

    #[test]
    fn all_worlds_for_world_yields_only_itself() {
        let helper: WorldHelper = sample_world_data().into();
        let w = helper.lookup_selector(AnySelector::World(100)).unwrap();
        let ids: Vec<_> = w.all_worlds().map(|w| w.id).collect();
        assert_eq!(ids, vec![100]);
    }

    #[test]
    fn get_datacenters_for_region_returns_all_datacenters() {
        let helper: WorldHelper = sample_world_data().into();
        let region = helper.lookup_selector(AnySelector::Region(1)).unwrap();
        let ids: Vec<_> = helper
            .get_datacenters(&region)
            .iter()
            .map(|d| d.id)
            .collect();
        assert_eq!(ids, vec![10, 11]);
    }

    #[test]
    fn get_datacenters_for_world_returns_owning_dc() {
        let helper: WorldHelper = sample_world_data().into();
        let world = helper.lookup_selector(AnySelector::World(110)).unwrap();
        let dcs = helper.get_datacenters(&world);
        assert_eq!(dcs.len(), 1);
        assert_eq!(dcs[0].id, 11);
    }

    #[test]
    fn get_region_for_world_returns_owning_region() {
        let helper: WorldHelper = sample_world_data().into();
        let world = helper.lookup_selector(AnySelector::World(200)).unwrap();
        let region = helper.get_region(world);
        assert_eq!(region.id, 2);
    }

    #[test]
    fn get_region_for_datacenter_returns_owning_region() {
        let helper: WorldHelper = sample_world_data().into();
        let dc = helper.lookup_selector(AnySelector::Datacenter(11)).unwrap();
        let region = helper.get_region(dc);
        assert_eq!(region.id, 1);
    }

    #[test]
    fn iter_yields_regions_datacenters_and_worlds_each() {
        let helper: WorldHelper = sample_world_data().into();
        let mut regions = 0;
        let mut datacenters = 0;
        let mut worlds = 0;
        for any in helper.iter() {
            match any {
                AnyResult::Region(_) => regions += 1,
                AnyResult::Datacenter(_) => datacenters += 1,
                AnyResult::World(_) => worlds += 1,
            }
        }
        assert_eq!(regions, 2);
        assert_eq!(datacenters, 3);
        assert_eq!(worlds, 4);
    }

    #[test]
    fn any_result_selector_conversion_matches_id() {
        let helper: WorldHelper = sample_world_data().into();
        let world = helper.lookup_selector(AnySelector::World(101)).unwrap();
        assert_eq!(AnySelector::from(&world), AnySelector::World(101));
        let dc = helper.lookup_selector(AnySelector::Datacenter(11)).unwrap();
        assert_eq!(AnySelector::from(&dc), AnySelector::Datacenter(11));
        let region = helper.lookup_selector(AnySelector::Region(2)).unwrap();
        assert_eq!(AnySelector::from(&region), AnySelector::Region(2));
    }

    #[test]
    fn regions_ordered_moves_preferred_region_to_front() {
        let helper: WorldHelper = sample_world_data().into();
        // No preference → original order.
        let names: Vec<_> = helper
            .regions_ordered(None)
            .iter()
            .map(|r| r.name.as_str())
            .collect();
        assert_eq!(names, vec!["North-America", "Japan"]);
        // Japan preferred → Japan first.
        let names: Vec<_> = helper
            .regions_ordered(Some("Japan"))
            .iter()
            .map(|r| r.name.as_str())
            .collect();
        assert_eq!(names, vec!["Japan", "North-America"]);
        // Unknown preference → original order (stable).
        let names: Vec<_> = helper
            .regions_ordered(Some("Atlantis"))
            .iter()
            .map(|r| r.name.as_str())
            .collect();
        assert_eq!(names, vec!["North-America", "Japan"]);
    }

    #[test]
    fn iter_with_region_priority_groups_under_preferred_region_first() {
        let helper: WorldHelper = sample_world_data().into();
        let mut region_order = vec![];
        for any in helper.iter_with_region_priority(Some("Japan")) {
            if let AnyResult::Region(r) = any {
                region_order.push(r.name.as_str());
            }
        }
        assert_eq!(region_order, vec!["Japan", "North-America"]);
    }

    #[test]
    fn owned_result_get_name_matches_inner() {
        let helper: WorldHelper = sample_world_data().into();
        let world = helper.lookup_selector(AnySelector::World(100)).unwrap();
        let owned: OwnedResult = world.into();
        assert_eq!(owned.get_name(), "Adamantoise");
        // round-trip ref
        assert_eq!(owned.as_ref().get_name(), "Adamantoise");
        // into selector
        assert_eq!(AnySelector::from(owned), AnySelector::World(100));
    }
}
