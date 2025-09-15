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

#[component]
pub fn LargeLoading(#[prop(into)] pending: Signal<bool>) -> impl IntoView {
    view! {
        <div
            class:opacity-50=pending
            class:opacity-0=move || !pending()
            class="bg-[color:color-mix(in_srgb,_var(--brand-ring)_22%,_var(--color-background))] absolute left-0 right-0 z-40 transition ease-in-out delay-250"
        >
            <div class="flex items-center justify-center py-6">
                <Loading />
            </div>
        </div>
    }
}
