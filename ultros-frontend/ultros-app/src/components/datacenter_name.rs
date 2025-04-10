use leptos::{either::Either, prelude::*};
use ultros_api_types::world_helper::AnySelector;

use crate::global_state::LocalWorldData;

use super::world_name::*;

#[component]
pub(crate) fn DatacenterName(world_id: i32) -> impl IntoView {
    match use_context::<LocalWorldData>()
        .expect("Local world data must be verified")
        .0
    {
        Ok(data) => {
            if let Some(world) = data.lookup_selector(AnySelector::World(world_id)) {
                let world = match world {
                    ultros_api_types::world_helper::AnyResult::World(world) => world,
                    _ => unreachable!("World cannot return non world"),
                };

                Either::Left(
                    view! { <WorldName id=AnySelector::Datacenter(world.datacenter_id) /> },
                )
            } else {
                Either::Right(view! { "" })
            }
        }
        _ => Either::Right(view! { "" }),
    }
    .into_any()
}
