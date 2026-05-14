use crate::components::icon::Icon;
use crate::i18n::{t, use_i18n};
use leptos::prelude::*;
use leptos_router::*;

#[component]
pub fn ListsNav() -> impl IntoView {
    let i18n = use_i18n();
    view! {<div class="content-nav">

        <A exact=true attr:class="nav-link" href="/list">
            <Icon height="1.25em" width="1.25em" icon=AiOrderedListOutlined />
            <span>{t!(i18n, lists)}</span>
        </A>
    </div>}
    .into_any()
}
