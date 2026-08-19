//! Region-name lookup shared by analyzer pages.
//!
//! Resolves a world name (typically from a route param or query string) to the region it
//! belongs to. Falls back to the user's home world's region, then to North-America if neither
//! is set. Lives here rather than per-page because the analyzer pages all need exactly this
//! string to feed `get_cheapest_listings(&region)` and the home-world / cache reads have to
//! happen in a tracked context.

use leptos::prelude::*;
use ultros_api_types::world_helper::AnyResult;

use crate::error::AppError;
use crate::global_state::{
    LocalWorldData, home_world::use_home_world, local_world_data::world_helper_from_context,
    use_world_helper,
};

const DEFAULT_REGION: &str = "North-America";

/// Resolves the region owning `world_name`, reporting failure instead of falling back.
///
/// The counterpart to [`use_region_for_world`], for callers that must not silently
/// substitute another region's prices — the Flip Finder keys its region-wide board off the
/// `:world` route param, so guessing would show one region's numbers under another's name.
/// Those callers want the page's existing error state instead.
///
/// Takes its inputs by value rather than reading context itself, so the resolution is a
/// plain function: no reactive runtime needed to test it, and every failure mode is a
/// return value. That matters because the only caller runs this inside a `Memo`, where a
/// panic is unusually destructive — `reactive_graph` *takes* a memo's cached value before
/// running the body, so an unwind leaves the memo permanently `None` and every subsequent
/// read panics in `try_read_untracked` (`arc_memo.rs:334`) rather than at the original
/// fault. One bad unwrap here takes down the whole page, repeatedly.
pub fn region_for_world_name(
    world_data: Option<LocalWorldData>,
    world_name: Option<String>,
) -> Result<String, AppError> {
    let worlds = world_helper_from_context(world_data)?;
    let world_name = world_name.ok_or(AppError::ParamMissing)?;
    worlds
        .lookup_world_by_name(&world_name)
        .map(|world| {
            let region = worlds.get_region(world);
            AnyResult::Region(region).get_name().to_string()
        })
        .ok_or(AppError::ParamMissing)
}

/// Returns a reactive `Memo<String>` of the region name for `world_name_source`.
///
/// `world_name_source` is typically a closure over a route-param signal or a query-string
/// signal (anything that yields `Option<String>` reactively). When the source returns
/// `None`, the user's home world is consulted; if that is also `None`, the default region
/// (`"North-America"`) is returned. The result is suitable to feed directly to
/// `get_cheapest_listings`.
pub fn use_region_for_world<F>(world_name_source: F) -> Memo<String>
where
    F: Fn() -> Option<String> + 'static + Send + Sync,
{
    let (home_world, _) = use_home_world();
    Memo::new(move |_| {
        let Ok(worlds) = use_world_helper() else {
            return DEFAULT_REGION.to_string();
        };

        let world_name = world_name_source()
            .or_else(|| home_world.get().map(|w| w.name))
            .unwrap_or_else(|| DEFAULT_REGION.to_string());

        worlds
            .lookup_world_by_name(&world_name)
            .map(|world| {
                let region = worlds.get_region(world);
                AnyResult::Region(region).get_name().to_string()
            })
            .unwrap_or_else(|| DEFAULT_REGION.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use ultros_api_types::world::{Datacenter, Region, World, WorldData};
    use ultros_api_types::world_helper::WorldHelper;

    fn worlds() -> LocalWorldData {
        LocalWorldData(Ok(Arc::new(WorldHelper::new(WorldData {
            regions: vec![Region {
                id: 1,
                name: "North-America".to_string(),
                datacenters: vec![Datacenter {
                    id: 10,
                    name: "Aether".to_string(),
                    region_id: 1,
                    worlds: vec![World {
                        id: 100,
                        name: "Gilgamesh".to_string(),
                        datacenter_id: 10,
                    }],
                }],
            }],
        }))))
    }

    #[test]
    fn resolves_the_region_owning_a_known_world() {
        assert_eq!(
            region_for_world_name(Some(worlds()), Some("Gilgamesh".to_string())),
            Ok("North-America".to_string())
        );
    }

    /// A failed `/api/v1/world_data` fetch stores `LocalWorldData(Err(_))`. This used to be
    /// `.0.unwrap()` inside the Flip Finder's `region` memo, so the fetch failing took the
    /// page's wasm bundle down instead of showing its error state.
    #[test]
    fn failed_world_data_is_an_error_not_a_panic() {
        assert_eq!(
            region_for_world_name(
                Some(LocalWorldData::failed("world data fetch failed")),
                Some("Gilgamesh".to_string())
            ),
            Err(AppError::WorldDataUnavailable)
        );
    }

    /// Likewise `use_context::<LocalWorldData>()` returning `None` — previously an
    /// `.expect("Worlds should always be populated here")`.
    #[test]
    fn missing_world_data_context_is_an_error_not_a_panic() {
        assert_eq!(
            region_for_world_name(None, Some("Gilgamesh".to_string())),
            Err(AppError::WorldDataUnavailable)
        );
    }

    #[test]
    fn a_missing_world_param_is_reported() {
        assert_eq!(
            region_for_world_name(Some(worlds()), None),
            Err(AppError::ParamMissing)
        );
    }

    /// An unknown world name stays an error rather than falling back to a default region:
    /// showing another region's prices under this one's name would be worse than an error.
    #[test]
    fn an_unknown_world_is_reported_rather_than_defaulted() {
        assert_eq!(
            region_for_world_name(Some(worlds()), Some("Nonexistent".to_string())),
            Err(AppError::ParamMissing)
        );
    }
}
