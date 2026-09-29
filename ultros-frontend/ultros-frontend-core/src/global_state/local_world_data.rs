use std::sync::Arc;

use leptos::prelude::use_context;
use ultros_api_types::world_helper::{AnySelector, WorldHelper};

use crate::error::{AppError, AppResult, SystemError};
#[derive(Clone)]
pub struct LocalWorldData(pub AppResult<Arc<WorldHelper>>);

impl LocalWorldData {
    pub fn failed(message: impl Into<String>) -> Self {
        Self(Err(AppError::SystemError(SystemError::Message(
            message.into(),
        ))))
    }
}

/// Resolves the world list from an optional `LocalWorldData` context value.
///
/// Both "the context was never provided" and "the context holds an `Err`" collapse to
/// [`AppError::WorldDataUnavailable`], which is exactly what that variant documents. Callers
/// only ever need to know whether a world/datacenter/region name can be resolved.
///
/// Kept as a plain function over an `Option` rather than reading context itself so the
/// resolution is testable without a reactive runtime, and so every failure mode is a return
/// value. Both states are reachable in production: `LocalWorldData(Err(_))` is what
/// `ultros-client`'s bootstrap deliberately stores when `/api/v1/world_data` fails (see
/// [`LocalWorldData::failed`]), and an absent context is what the server hit in GlitchTip
/// #7120/#7187. Every call site below used to `.expect()`/`.unwrap()` one of the two while
/// already carrying correct handling for the other.
pub fn world_helper_from_context(data: Option<LocalWorldData>) -> AppResult<Arc<WorldHelper>> {
    data.and_then(|data| data.0.ok())
        .ok_or(AppError::WorldDataUnavailable)
}

/// [`world_helper_from_context`] over the ambient `LocalWorldData` context.
pub fn use_world_helper() -> AppResult<Arc<WorldHelper>> {
    world_helper_from_context(use_context())
}

/// Display name for `selector`, or `None` when the world list is unavailable or the selector
/// names nothing in it. Pure counterpart of [`use_world_display_name`].
pub fn world_display_name_from_context(
    data: Option<LocalWorldData>,
    selector: AnySelector,
) -> Option<String> {
    world_helper_from_context(data).ok().and_then(|worlds| {
        worlds
            .lookup_selector(selector)
            .map(|result| result.get_name().to_string())
    })
}

/// [`world_display_name_from_context`] over the ambient `LocalWorldData` context.
pub fn use_world_display_name(selector: AnySelector) -> Option<String> {
    world_display_name_from_context(use_context(), selector)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultros_api_types::world::{Datacenter, Region, World, WorldData};

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

    /// The server renders `WorldName` with no `LocalWorldData` in scope
    /// (GlitchTip #7120/#7187, `panic.location` = `components/world_name.rs:11:10`). That was
    /// an `.expect("Local world data must be verified")`, i.e. an unhandled panic mid-SSR,
    /// even though the component already had a fallback arm for unusable world data.
    #[test]
    fn missing_world_data_context_is_an_error_not_a_panic() {
        assert_eq!(
            world_helper_from_context(None).map(|_| ()),
            Err(AppError::WorldDataUnavailable)
        );
    }

    /// A failed `/api/v1/world_data` fetch is a *designed* state on the client — the bootstrap
    /// stores `LocalWorldData::failed(..)` rather than giving up — so unwrapping it took the
    /// whole wasm bundle down on a transient network error.
    #[test]
    fn failed_world_data_is_an_error_not_a_panic() {
        assert_eq!(
            world_helper_from_context(Some(LocalWorldData::failed("world data fetch failed")))
                .map(|_| ()),
            Err(AppError::WorldDataUnavailable)
        );
    }

    #[test]
    fn a_loaded_world_list_resolves() {
        assert!(world_helper_from_context(Some(worlds())).is_ok());
    }

    #[test]
    fn display_names_resolve_for_every_selector_kind() {
        assert_eq!(
            world_display_name_from_context(Some(worlds()), AnySelector::World(100)),
            Some("Gilgamesh".to_string())
        );
        assert_eq!(
            world_display_name_from_context(Some(worlds()), AnySelector::Datacenter(10)),
            Some("Aether".to_string())
        );
        assert_eq!(
            world_display_name_from_context(Some(worlds()), AnySelector::Region(1)),
            Some("North-America".to_string())
        );
    }

    /// An id that isn't in the list is indistinguishable, to the caller, from no list at all:
    /// there is no name to show either way.
    #[test]
    fn an_unknown_world_id_has_no_display_name() {
        assert_eq!(
            world_display_name_from_context(Some(worlds()), AnySelector::World(999)),
            None
        );
    }

    /// The retainer pages formatted their `<title>` off this lookup after `.0.unwrap()`.
    #[test]
    fn unusable_world_data_has_no_display_name() {
        assert_eq!(
            world_display_name_from_context(None, AnySelector::World(100)),
            None
        );
        assert_eq!(
            world_display_name_from_context(
                Some(LocalWorldData::failed("world data fetch failed")),
                AnySelector::World(100)
            ),
            None
        );
    }
}
