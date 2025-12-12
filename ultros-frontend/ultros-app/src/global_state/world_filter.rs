use std::collections::HashSet;

use leptos::prelude::*;
use ultros_api_types::world_helper::AnySelector;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorldFilter(pub(crate) HashSet<AnySelector>);

impl WorldFilter {
}

pub(crate) fn provide_world_filter_context() {
    provide_context(RwSignal::new(WorldFilter(HashSet::new())));
}
