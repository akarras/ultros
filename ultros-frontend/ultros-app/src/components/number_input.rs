use std::str::FromStr;

use leptos::*;
use web_sys::wasm_bindgen::JsValue;

#[component]
pub fn ParseableInputBox<T>(
    #[prop(into)] input: Signal<T>,
    #[prop(into)] set_value: SignalSetter<T>,
) -> impl IntoView
where
    T: FromStr + IntoProperty + Clone + Into<JsValue> + 'static,
{
    let failed_to_parse = create_rw_signal(false);
    view! {
        <input class=move || {
            if failed_to_parse() {
                "border-2 border-red-950 rounded"
            } else {
                "border rounded border-violet-950"
            }
        } prop:value=input on:input=move |e| {
            if let Ok(e) = event_target_value(&e).parse() {
                failed_to_parse.set(false);
                set_value(e);
            } else {
                failed_to_parse.set(true);
            }

        } />
    }
}
