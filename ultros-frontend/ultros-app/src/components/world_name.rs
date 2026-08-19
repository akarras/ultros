use leptos::{either::Either, prelude::*};
use ultros_api_types::world_helper::AnySelector;

use crate::global_state::use_world_helper;
use crate::i18n::{t, use_i18n};

#[component]
pub(crate) fn WorldName(id: AnySelector) -> impl IntoView {
    let i18n = use_i18n();
    // An absent context is as recoverable as a failed world-data fetch, and both land in the
    // `none_label` arm below. Panicking on the former aborted the SSR stream mid-response
    // (GlitchTip #7120/#7187).
    match use_world_helper() {
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
