use leptos::*;
use ultros_api_types::world_helper::AnySelector;

use crate::global_state::LocalWorldData;

use super::world_name::*;

#[component]
pub(crate) fn DatacenterName(cx: Scope, world_id: i32) -> impl IntoView {
    let context = use_context::<LocalWorldData>(cx).expect("Local world data must be verified");
    view! {
        cx,
        <Suspense fallback=|| view!{cx, "--"}>
            {move ||
                match context.0.read(cx) {
                    Some(Some(data)) => if let Some(world) = data.lookup_selector(AnySelector::World(world_id)) {
                        let world = match world {
                            ultros_api_types::world_helper::AnyResult::World(world) => world,
                            _ => unreachable!("World cannot return non world")
                        };

                        view!{ cx, <WorldName id=AnySelector::Datacenter(world.datacenter_id)/>}.into_view(cx)
                    } else {
                        view!{ cx, ""}.into_view(cx)
                    },
                    _ => {view!{ cx, ""}.into_view(cx)}
                }
            }
        </Suspense>
    }
}
