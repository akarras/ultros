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
                      bg-violet-950/60 border border-white/10
                      hover:border-violet-400/30
                      peer-checked:bg-violet-900/60
                      peer-checked:border-violet-400/20
                      peer-focus:ring-2 peer-focus:ring-violet-500/30
                      backdrop-blur-sm">

                <div class="absolute top-0.5 left-0.5
                           w-5 h-5 rounded-full
                           transition-all duration-300 ease-in-out transform bg-gradient-to-br
                           group-hover:from-amber-100 group-hover:to-amber-200
                           shadow-md"
                           class=(["translate-x-0", "from-gray-200", "to-gray-300"], move || !checked())
                           class=(["translate-x-6", "to-amber-300", "from-amber-200"], move || checked())
                >
                    <div class="absolute inset-[15%] rounded-full
                              bg-gradient-to-br from-white/80 to-transparent">
                    </div>
                </div>
            </div>

            <span class="text-sm font-medium text-gray-300 transition-colors duration-300
                        group-hover:text-amber-200">
                {move || {
                    if checked() {
                        checked_label.to_string()
                    } else {
                        unchecked_label.to_string()
                    }
                }}
            </span>
        </label>
    }.into_any()
}
