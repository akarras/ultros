use leptos::{either::Either, prelude::*};
use ultros_api_types::world_helper::AnySelector;

use crate::global_state::LocalWorldData;
use crate::i18n::{t, use_i18n};

#[component]
pub(crate) fn WorldName(id: AnySelector) -> impl IntoView {
    let i18n = use_i18n();
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
        _ => Either::Right(view! { <span>{t!(i18n, none_label)}</span> }),
    }
    .into_any()
}
