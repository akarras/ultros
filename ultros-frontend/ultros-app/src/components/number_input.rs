use std::str::FromStr;

use leptos::*;
use web_sys::wasm_bindgen::JsValue;

#[component]
pub fn ParseableInputBox<T>(
    #[prop(into)] input: Signal<Option<T>>,
    #[prop(into)] set_value: SignalSetter<Option<T>>,
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
        } prop:value=move || input().map(|value| value.into()).unwrap_or(JsValue::NULL) on:input=move |e| {
            let value = event_target_value(&e);
            if value.is_empty() {
                set_value(None);
                failed_to_parse.set(false);
                return;
            }
            if let Ok(e) = value.parse() {
                failed_to_parse.set(false);
                set_value(Some(e));
            } else {
                failed_to_parse.set(true);
            }


        } />
    }
}
