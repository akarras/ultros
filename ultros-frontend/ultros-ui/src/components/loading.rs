use crate::components::icon::Icon;
use icondata as i;
use leptos::prelude::*;

#[component]
pub fn Loading() -> impl IntoView {
    view! {
        <div role="status" aria-live="polite" class="inline-flex items-center gap-2 text-[color:var(--color-text)]">
            <Icon icon=i::FaSpaghettiMonsterFlyingSolid attr:class="w-5 h-5 animate-spin" />
            <span class="sr-only">Loading</span>
        </div>
    }
    .into_any()
}
