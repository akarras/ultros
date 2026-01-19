use std::str::FromStr;

use leptos::{
    prelude::*, reactive::wrappers::write::SignalSetter, tachys::html::property::IntoProperty,
};
use web_sys::wasm_bindgen::JsValue;

#[component]
pub fn ParseableInputBox<T>(
    #[prop(into)] input: Signal<Option<T>>,
    #[prop(into)] set_value: SignalSetter<Option<T>>,
    #[prop(optional, into)] id: Option<String>,
    #[prop(optional, into)] placeholder: Option<String>,
    #[prop(optional, into)] aria_label: Option<String>,
    #[prop(optional, into)] class: Option<String>,
    #[prop(optional, into)] input_type: Option<String>,
) -> impl IntoView
where
    T: FromStr + IntoProperty + Clone + Into<JsValue> + Send + Sync + 'static,
{
    let failed_to_parse = RwSignal::new(false);
    let class = class.unwrap_or_default();
    let input_type = input_type.unwrap_or("text".to_string());

    view! {
        <input
            id=id
            type=input_type
            placeholder=placeholder
            aria-label=aria_label
            aria-invalid=move || failed_to_parse().to_string()
            class=move || {
                let base_class = if failed_to_parse() {
                    "input w-full border-red-600/40 focus-visible:ring-red-500/30"
                } else {
                    "input w-full"
                };
                if class.is_empty() {
                    base_class.to_string()
                } else {
                    format!("{} {}", base_class, class)
                }
            }

            prop:value=move || input().map(|value| value.into()).unwrap_or(JsValue::NULL)
            on:input=move |e| {
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
            }
        />
    }
    .into_any()
}
