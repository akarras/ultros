use leptos::{either::Either, prelude::*};
use ultros_api_types::world_helper::AnySelector;

use crate::global_state::LocalWorldData;

#[component]
pub(crate) fn WorldName(id: AnySelector) -> impl IntoView {
    match use_context::<LocalWorldData>()
        .expect("Local world data must be verified")
        .0
    {
        Ok(data) => Either::Left(view! {
            <span>
                {data
                    .lookup_selector(id)
                    .map(|value| value.get_name().to_string())
                    .unwrap_or_default()}
            </span>
        }),
        _ => Either::Right(view! { <span>"None"</span> }),
    }
    .into_any()
}

