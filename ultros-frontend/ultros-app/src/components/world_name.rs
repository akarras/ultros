use leptos::*;
use ultros_api_types::world_helper::AnySelector;

use crate::global_state::LocalWorldData;

#[component]
pub(crate) fn WorldName(cx: Scope, id: AnySelector) -> impl IntoView {
    let context = use_context::<LocalWorldData>(cx).expect("Local world data must be verified");
    view! {
        cx,
        <Suspense fallback=|| view!{cx, "--"}>
            {move ||
                    match use_context::<LocalWorldData>(cx).expect("Local world data must be verified").0 {
                        Ok(data) => view!{ cx, <span>{data.lookup_selector(id).map(|value| value.get_name().to_string()).unwrap_or_default()}</span> }.into_view(cx),
                        _ => view!{ cx, <span>"None"</span>}.into_view(cx),
                    }
                }
        </Suspense>
    }
}
