//! Debug-only deterministic coverage for shared keyboard interactions.
use leptos::prelude::*;
use ultros_ui::components::{
    modal::Modal, reorderable_list::ReorderableList, select::Select, tooltip::Tooltip,
};

#[component]
pub fn AccessibilityFixture() -> impl IntoView {
    let open = RwSignal::new(false);
    let toasts = ultros_ui::global_state::toasts::use_toast().expect("toast context");
    let chosen = RwSignal::new(Some("Alpha".to_string()));
    let second = RwSignal::new(Some("Beta".to_string()));
    let rows = RwSignal::new(vec![
        "Alpha".to_string(),
        "Beta".to_string(),
        "Gamma".to_string(),
    ]);
    let choices = Signal::derive(|| vec!["Alpha".to_string(), "Beta".to_string()]);
    view! {
        <h1>"Accessibility fixture"</h1>
        <button id="open-fixture" class="btn-primary" on:click=move |_| open.set(true)>"Open task dialog"</button>
        <button id="fixture-error-toast" on:click=move |_| toasts.error("Fixture error")>"Show error"</button>
        <ReorderableList items=rows key_fn=|row: &String| row.clone() item_label=|row: &String| row.clone()
            item_view=|row| view! { <span class="fixture-row-name">{row}</span> }/>
        <Show when=move || open.get()>
            <Modal set_visible=open.write_only()>
                <h2>"Choose worlds"</h2>
                <button id="fixture-dialog-error" on:click=move |_| toasts.error("Dialog fixture error")>"Show dialog error"</button>
                <Tooltip tooltip_text="Pick a world for each market"><span id="fixture-help">"World help"</span></Tooltip>
                <label for="fixture-buy-world">"Buy world"</label>
                <Select input_id=Some("fixture-buy-world".to_string()) label="Buy world" items=choices choice=chosen.into() set_choice=chosen.write_only().into()
                    as_label=|value: &String| value.clone() children=|_, label| view! { <span>{label}</span> }/>
                <Select label="Sell world" items=choices choice=second.into() set_choice=second.write_only().into()
                    as_label=|value: &String| value.clone() children=|_, label| view! { <span>{label}</span> }/>
                <button class="btn-secondary" on:click=move |_| open.set(false)>"Done"</button>
            </Modal>
        </Show>
    }
}
