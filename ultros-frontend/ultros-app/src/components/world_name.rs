use leptos::*;
use ultros_api_types::world_helper::AnySelector;

use crate::global_state::LocalWorldData;

#[component]
pub(crate) fn WorldName(id: AnySelector) -> impl IntoView {
    view! {

        <Suspense fallback=|| view!{"--"}>
            {move ||
                    match use_context::<LocalWorldData>().expect("Local world data must be verified").0 {
                        Ok(data) => view!{ <span>{data.lookup_selector(id).map(|value| value.get_name().to_string()).unwrap_or_default()}</span> }.into_view(),
                        _ => view!{ <span>"None"</span>}.into_view(),
                    }
                }
        </Suspense>
    }
}
