use leptos::prelude::*;

#[component]
pub fn Loading() -> impl IntoView {
    view! {
        <div role="status" aria-live="polite" class="inline-flex items-center gap-2 text-[color:var(--color-text)]">
            <div class="w-5 h-5 border-2 border-[color:color-mix(in_srgb,var(--brand-ring)_40%,transparent)] border-t-transparent rounded-full animate-spin"></div>
            <span class="sr-only">Loading</span>
        </div>
    }
    .into_any()
}
