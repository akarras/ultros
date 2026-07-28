//! `GroupLevel`: which level of the world hierarchy the chart may request,
//! and its wire-format conversion to/from `SeriesGroup`. Sales are grouped
//! and bucketed server-side now (see `ultros_api_types::price_series`); this
//! module only tracks which grouping levels a given scope page may offer.

use ultros_api_types::world_helper::WorldHelper;

/// Which level of the world hierarchy to roll sales up to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupLevel {
    Region,
    Datacenter,
    World,
}

impl GroupLevel {
    /// Stable identifier (list keys / debugging); user-facing names come
    /// from the app's i18n layer.
    pub fn label(self) -> &'static str {
        match self {
            Self::Region => "Region",
            Self::Datacenter => "Datacenter",
            Self::World => "World",
        }
    }
}

impl From<GroupLevel> for ultros_api_types::price_series::SeriesGroup {
    fn from(level: GroupLevel) -> Self {
        match level {
            GroupLevel::Region => Self::Region,
            GroupLevel::Datacenter => Self::Datacenter,
            GroupLevel::World => Self::World,
        }
    }
}

impl From<ultros_api_types::price_series::SeriesGroup> for GroupLevel {
    fn from(group: ultros_api_types::price_series::SeriesGroup) -> Self {
        use ultros_api_types::price_series::SeriesGroup;
        match group {
            SeriesGroup::Region => Self::Region,
            SeriesGroup::Datacenter => Self::Datacenter,
            SeriesGroup::World => Self::World,
        }
    }
}

/// Which grouping levels make sense for the scope page being viewed —
/// ported from the web UI (a world page only offers World; a DC page offers
/// DC + World; a region page or unknown scope offers everything).
pub fn available_group_levels(world_helper: &WorldHelper, scope_name: &str) -> Vec<GroupLevel> {
    match world_helper.lookup_world_by_name(scope_name) {
        Some(result) if result.as_world().is_some() => vec![GroupLevel::World],
        Some(result) if result.as_datacenter().is_some() => {
            vec![GroupLevel::Datacenter, GroupLevel::World]
        }
        _ => vec![
            GroupLevel::Region,
            GroupLevel::Datacenter,
            GroupLevel::World,
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::world_helper;
    use ultros_api_types::price_series::SeriesGroup;

    #[test]
    fn available_levels_follow_the_viewed_scope() {
        let h = world_helper();
        assert_eq!(
            available_group_levels(&h, "Gilgamesh"),
            vec![GroupLevel::World]
        );
        assert_eq!(
            available_group_levels(&h, "Aether"),
            vec![GroupLevel::Datacenter, GroupLevel::World]
        );
        assert_eq!(
            available_group_levels(&h, "North-America"),
            vec![
                GroupLevel::Region,
                GroupLevel::Datacenter,
                GroupLevel::World
            ]
        );
        assert_eq!(
            available_group_levels(&h, "Not A Scope"),
            vec![
                GroupLevel::Region,
                GroupLevel::Datacenter,
                GroupLevel::World
            ]
        );
    }

    #[test]
    fn group_level_round_trips_through_series_group() {
        for level in [
            GroupLevel::Region,
            GroupLevel::Datacenter,
            GroupLevel::World,
        ] {
            assert_eq!(GroupLevel::from(SeriesGroup::from(level)), level);
        }
    }

    // A round trip alone can't catch a *symmetric* mis-mapping: if
    // `From<GroupLevel>` sent `Region => Datacenter` while `From<SeriesGroup>`
    // sent `Datacenter => Region`, `group_level_round_trips_through_series_group`
    // above would still pass (Region -> Datacenter -> Region), even though every
    // API request would carry the wrong grouping. Pin the forward mapping
    // absolutely so that class of bug can't hide behind the round trip.
    #[test]
    fn group_level_maps_to_the_matching_series_group() {
        assert_eq!(SeriesGroup::from(GroupLevel::Region), SeriesGroup::Region);
        assert_eq!(
            SeriesGroup::from(GroupLevel::Datacenter),
            SeriesGroup::Datacenter
        );
        assert_eq!(SeriesGroup::from(GroupLevel::World), SeriesGroup::World);
    }
}
