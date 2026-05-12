//! Region-name lookup shared by analyzer pages.
//!
//! Resolves a world name (typically from a route param or query string) to the region it
//! belongs to. Falls back to the user's home world's region, then to North-America if neither
//! is set. Lives here rather than per-page because the analyzer pages all need exactly this
//! string to feed `get_cheapest_listings(&region)` and the home-world / cache reads have to
//! happen in a tracked context.

use leptos::prelude::*;
use ultros_api_types::world_helper::AnyResult;

use crate::global_state::{LocalWorldData, home_world::use_home_world};

const DEFAULT_REGION: &str = "North-America";

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
        let Some(worlds) = use_context::<LocalWorldData>().and_then(|d| d.0.ok()) else {
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
