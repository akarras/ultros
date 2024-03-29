use leptos::*;

#[component]
pub fn Toggle(
    #[prop(into)] checked: Signal<bool>,
    #[prop(into)] set_checked: SignalSetter<bool>,
    #[prop(into)] checked_label: Oco<'static, str>,
    #[prop(into)] unchecked_label: Oco<'static, str>,
) -> impl IntoView {
    view! {

        <label class="relative inline-flex items-top cursor-pointer">
            <input type="checkbox" value="" class="sr-only peer" prop:checked=checked on:change=move |_| {
                let checked = checked.get_untracked();
                set_checked(!checked);
            } />
            <div class="w-11 h-6 peer-focus:outline-none peer-focus:ring-4 peer-focus:ring-blue-800 rounded-full peer bg-gray-700
             peer-checked:after:translate-x-full peer-checked:after:border-white after:content-['']
             after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-gray-300 after:border
             after:rounded-full after:h-5 after:w-5 after:transition-all border-gray-600 peer-checked:bg-blue-600"></div>
            <span class="ml-3 text-sm font-medium text-gray-300">{move || if checked() { checked_label.to_string() } else { unchecked_label.to_string() }}</span>
        </label>

    }
}
