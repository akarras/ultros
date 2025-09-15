use leptos::{prelude::*, reactive::wrappers::write::SignalSetter};

#[component]
pub fn Toggle(
    #[prop(into)] checked: Signal<bool>,
    #[prop(into)] set_checked: SignalSetter<bool>,
    #[prop(into)] checked_label: Oco<'static, str>,
    #[prop(into)] unchecked_label: Oco<'static, str>,
) -> impl IntoView {
    view! {
        <label class="relative inline-flex items-center gap-3 cursor-pointer group">
            <input
                type="checkbox"
                class="sr-only peer"
                prop:checked=checked
                on:change=move |_| {
                    let checked = checked.get_untracked();
                    set_checked(!checked);
                }
            />

            <div class="w-12 h-6 rounded-full relative
            transition-all duration-300 ease-in-out
            border
            bg-[color:color-mix(in_srgb,var(--color-text)_8%,transparent)]
            border-[color:var(--color-outline)]
            hover:border-[color:color-mix(in_srgb,var(--brand-ring)_30%,var(--color-outline))]
            peer-checked:bg-[color:color-mix(in_srgb,var(--brand-ring)_28%,transparent)]
            peer-checked:border-[color:color-mix(in_srgb,var(--brand-ring)_40%,var(--color-outline))]
            peer-focus:outline-none peer-focus:ring-2
            peer-focus:ring-[color:color-mix(in_srgb,var(--brand-ring)_35%,transparent)]
            ">

                <div
                    class="absolute top-[1px] left-0.5
                    w-6 h-[22px] rounded-full
                    transition-all duration-300 ease-in-out transform
                    shadow-md border border-[color:var(--color-outline-strong)]
                    bg-[color:var(--color-background-elevated)]"
                    class=(["translate-x-0"], move || !checked())
                    class=(["translate-x-6"], move || checked())
                >
                </div>
            </div>

            <span class="text-sm font-medium text-[color:var(--color-text-muted)] transition-colors duration-300
            group-hover:text-[color:var(--color-text)]">
                {move || {
                    if checked() { checked_label.to_string() } else { unchecked_label.to_string() }
                }}
            </span>
        </label>
    }
    .into_any()
}
