//! Travel limits narrow fetched market data before either list estimator
//! allocates physical supply. This module never fetches or adds offers.
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    str::FromStr,
};
use ultros_api_types::{ActiveListing, world_helper::WorldHelper};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TravelLimit {
    #[default]
    Scope,
    World,
    Datacenter,
}

impl fmt::Display for TravelLimit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Scope => "scope",
            Self::World => "world",
            Self::Datacenter => "dc",
        })
    }
}

impl FromStr for TravelLimit {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "scope" => Ok(Self::Scope),
            "world" => Ok(Self::World),
            "dc" => Ok(Self::Datacenter),
            _ => Err("Unknown list travel limit"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TravelBlocked {
    WorldData,
    UnknownDatacenter,
    HomeWorld,
    HomeDatacenter,
}

/// World metadata must include the home world even when it lies outside the
/// fetched price scope. Exclusions and the selected limit always intersect.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TravelPolicy {
    /// An explicit DC exclusion cannot be resolved without the world catalog.
    pub metadata_missing: bool,
    pub unresolved_datacenters: BTreeSet<String>,
    pub limit: TravelLimit,
    pub home_world: Option<i32>,
    pub datacenters: BTreeMap<i32, i32>,
    pub excluded_worlds: BTreeSet<i32>,
    pub excluded_datacenters: BTreeSet<i32>,
}

impl TravelPolicy {
    /// Resolve the existing URL's named DC exclusions against the full world
    /// catalog. A narrower price scope must not hide the home world's DC.
    pub fn from_world_helper(
        limit: TravelLimit,
        home_world: Option<i32>,
        worlds: &WorldHelper,
        excluded_worlds: BTreeSet<i32>,
        excluded_datacenter_names: &BTreeSet<String>,
    ) -> Self {
        let datacenters: BTreeMap<_, _> = worlds
            .iter()
            .filter_map(|value| {
                value
                    .as_world()
                    .map(|world| (world.id, world.datacenter_id))
            })
            .collect();
        let known_names: BTreeSet<_> = worlds
            .iter()
            .filter_map(|value| value.as_datacenter().map(|dc| dc.name.clone()))
            .collect();
        Self {
            metadata_missing: datacenters.is_empty(),
            unresolved_datacenters: excluded_datacenter_names
                .difference(&known_names)
                .cloned()
                .collect(),
            limit,
            home_world,
            datacenters,
            excluded_worlds,
            excluded_datacenters: worlds
                .iter()
                .filter_map(|value| value.as_datacenter())
                .filter(|dc| excluded_datacenter_names.contains(&dc.name))
                .map(|dc| dc.id)
                .collect(),
        }
    }

    pub fn blocked(&self) -> Option<TravelBlocked> {
        if self.metadata_missing {
            return Some(TravelBlocked::WorldData);
        }
        if !self.unresolved_datacenters.is_empty() {
            return Some(TravelBlocked::UnknownDatacenter);
        }
        if self.limit == TravelLimit::Scope {
            return None;
        }
        let Some(home) = self.home_world.filter(|world| *world > 0) else {
            return Some(TravelBlocked::HomeWorld);
        };
        (self.limit == TravelLimit::Datacenter && !self.datacenters.contains_key(&home))
            .then_some(TravelBlocked::HomeDatacenter)
    }

    pub fn allows_world(&self, world: i32) -> bool {
        if world <= 0 || self.blocked().is_some() || self.excluded_worlds.contains(&world) {
            return false;
        }
        let dc = self.datacenters.get(&world);
        // With explicit DC exclusions, missing metadata cannot establish that
        // a world is allowed. Do not silently undo the user's constraint.
        if !self.excluded_datacenters.is_empty()
            && dc.is_none_or(|dc| self.excluded_datacenters.contains(dc))
        {
            return false;
        }
        match self.limit {
            TravelLimit::Scope => true,
            TravelLimit::World => self.home_world == Some(world),
            TravelLimit::Datacenter => {
                dc.is_some() && dc == self.home_world.and_then(|home| self.datacenters.get(&home))
            }
        }
    }

    /// Whether the policy narrows served supply at all. A plain fetched-scope
    /// policy with nothing excluded leaves every offer in place, even before
    /// the world catalog has loaded; only an actual limit or exclusion, or an
    /// exclusion that failed to resolve, applies and then fails closed.
    pub fn narrows(&self) -> bool {
        self.limit != TravelLimit::Scope
            || !self.excluded_worlds.is_empty()
            || !self.excluded_datacenters.is_empty()
            || !self.unresolved_datacenters.is_empty()
    }

    pub fn filter_listings(&self, listings: &[ActiveListing]) -> Vec<ActiveListing> {
        if !self.narrows() {
            return listings.to_vec();
        }
        listings
            .iter()
            .filter(|listing| self.allows_world(listing.world_id))
            .cloned()
            .collect()
    }

    /// Keep every cart row, including rows with no allowed supply. Removing
    /// those rows would make an impossible constrained route appear complete.
    pub fn filter_rows<T: Clone>(
        &self,
        rows: &[(T, Vec<ActiveListing>)],
    ) -> Vec<(T, Vec<ActiveListing>)> {
        rows.iter()
            .map(|(row, listings)| (row.clone(), self.filter_listings(listings)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> TravelPolicy {
        TravelPolicy {
            home_world: Some(9),
            datacenters: BTreeMap::from([(1, 10), (9, 10), (20, 30)]),
            ..Default::default()
        }
    }

    #[test]
    fn limits_and_exclusions_intersect_without_expanding_scope() {
        let mut p = policy();
        let worlds = [1, 9, 20];
        let allowed = |p: &TravelPolicy| {
            worlds
                .into_iter()
                .filter(|world| p.allows_world(*world))
                .collect::<Vec<_>>()
        };
        assert_eq!(allowed(&p), vec![1, 9, 20]);
        p.limit = TravelLimit::Datacenter;
        assert_eq!(allowed(&p), vec![1, 9]);
        p.excluded_worlds.insert(1);
        assert_eq!(allowed(&p), vec![9]);
        p.excluded_datacenters.insert(10);
        assert!(allowed(&p).is_empty());
        p.limit = TravelLimit::World;
        assert!(
            allowed(&p).is_empty(),
            "current world never bypasses an exclusion"
        );
        p.excluded_datacenters.clear();
        assert_eq!(allowed(&p), vec![9]);
        // A price scope that contains only world1 cannot gain home9 offers.
        assert!(![1].into_iter().any(|world| p.allows_world(world)));
    }

    #[test]
    fn missing_home_or_dc_never_falls_back_to_regional_travel() {
        let mut p = policy();
        p.limit = TravelLimit::World;
        p.home_world = None;
        assert_eq!(p.blocked(), Some(TravelBlocked::HomeWorld));
        assert!(!p.allows_world(1));
        p.home_world = Some(0);
        assert_eq!(p.blocked(), Some(TravelBlocked::HomeWorld));
        p.home_world = Some(99);
        p.limit = TravelLimit::Datacenter;
        assert_eq!(p.blocked(), Some(TravelBlocked::HomeDatacenter));
        assert!(!p.allows_world(1));
        p.limit = TravelLimit::Scope;
        assert_eq!(p.blocked(), None);
        assert!(p.allows_world(1));
        p.excluded_datacenters.insert(30);
        assert!(!p.allows_world(99));
        assert!(!p.allows_world(20));
    }

    #[test]
    fn plain_scope_passes_through_but_any_constraint_fails_closed_without_metadata() {
        let listing = ActiveListing {
            id: 1,
            world_id: 9,
            item_id: 42,
            retainer_id: 1,
            quantity: 3,
            price_per_unit: 10,
            hq: false,
            timestamp: "2026-09-16T00:00:00".parse().unwrap(),
        };
        let mut p = TravelPolicy {
            metadata_missing: true,
            ..Default::default()
        };
        assert!(!p.narrows());
        assert_eq!(p.blocked(), Some(TravelBlocked::WorldData));
        assert_eq!(
            p.filter_listings(std::slice::from_ref(&listing)),
            vec![listing.clone()]
        );
        p.excluded_worlds.insert(1);
        assert!(p.narrows());
        assert!(p.filter_listings(std::slice::from_ref(&listing)).is_empty());
        p.excluded_worlds.clear();
        p.unresolved_datacenters.insert("Renamed DC".into());
        assert!(p.narrows());
        assert!(p.filter_listings(&[listing]).is_empty());
    }

    #[test]
    fn constrained_rows_keep_missing_supply_and_preserve_offer_identity() {
        let listing = |id, world_id| ActiveListing {
            id,
            world_id,
            item_id: 42,
            retainer_id: 1,
            quantity: 3,
            price_per_unit: 10,
            hq: true,
            timestamp: "2026-09-16T00:00:00".parse().unwrap(),
        };
        let rows = vec![
            ("covered", vec![listing(1, 9), listing(2, 20)]),
            ("missing", vec![listing(3, 20)]),
            ("unpriced", vec![]),
        ];
        let p = TravelPolicy {
            limit: TravelLimit::World,
            ..policy()
        };
        let filtered = p.filter_rows(&rows);
        assert_eq!(filtered.len(), 3);
        assert_eq!(filtered[0].1, vec![rows[0].1[0].clone()]);
        assert!(filtered[1].1.is_empty());
        assert!(filtered[2].1.is_empty());
        assert_eq!(rows[0].1.len(), 2, "a frozen trip's source is not mutated");
    }

    #[test]
    fn query_values_round_trip_and_unknown_values_are_rejected() {
        for value in [
            TravelLimit::Scope,
            TravelLimit::World,
            TravelLimit::Datacenter,
        ] {
            assert_eq!(value.to_string().parse::<TravelLimit>(), Ok(value));
        }
        assert!("anywhere".parse::<TravelLimit>().is_err());
    }

    #[test]
    fn named_exclusions_use_full_metadata_even_outside_the_price_scope() {
        use ultros_api_types::world::{Datacenter, Region, World, WorldData};
        let worlds = WorldHelper::new(WorldData {
            regions: vec![Region {
                id: 1,
                name: "Region".into(),
                datacenters: vec![
                    Datacenter {
                        id: 10,
                        name: "Home DC".into(),
                        region_id: 1,
                        worlds: vec![
                            World {
                                id: 9,
                                name: "Home".into(),
                                datacenter_id: 10,
                            },
                            World {
                                id: 1,
                                name: "Neighbor".into(),
                                datacenter_id: 10,
                            },
                        ],
                    },
                    Datacenter {
                        id: 30,
                        name: "Other DC".into(),
                        region_id: 1,
                        worlds: vec![World {
                            id: 20,
                            name: "Foreign".into(),
                            datacenter_id: 30,
                        }],
                    },
                ],
            }],
        });
        let policy = TravelPolicy::from_world_helper(
            TravelLimit::Datacenter,
            Some(9),
            &worlds,
            BTreeSet::from([1]),
            &BTreeSet::from(["Other DC".into()]),
        );
        assert_eq!(policy.datacenters.get(&9), Some(&10));
        assert_eq!(policy.excluded_datacenters, BTreeSet::from([30]));
        assert!(policy.allows_world(9));
        assert!(!policy.allows_world(1));
        assert!(!policy.allows_world(20));
        assert_eq!(policy.blocked(), None);
        let unresolved = TravelPolicy::from_world_helper(
            TravelLimit::Scope,
            Some(9),
            &worlds,
            BTreeSet::new(),
            &BTreeSet::from(["Renamed DC".into(), "Other DC".into()]),
        );
        assert_eq!(unresolved.blocked(), Some(TravelBlocked::UnknownDatacenter));
        assert_eq!(
            unresolved.unresolved_datacenters,
            BTreeSet::from(["Renamed DC".into()])
        );
        assert_eq!(unresolved.excluded_datacenters, BTreeSet::from([30]));
        assert!(
            !unresolved.allows_world(9),
            "a stale exclusion never silently broadens the route"
        );
        let empty = TravelPolicy::from_world_helper(
            TravelLimit::Scope,
            Some(9),
            &WorldHelper::new(WorldData { regions: vec![] }),
            BTreeSet::new(),
            &BTreeSet::from(["Other DC".into()]),
        );
        assert_eq!(empty.blocked(), Some(TravelBlocked::WorldData));
        assert!(
            !empty.allows_world(9),
            "an uninitialized catalog cannot undo an exclusion"
        );
    }
}
