use crate::components::meta::{MetaDescription, MetaTitle};
use leptos::prelude::*;

#[component]
pub fn Alerts() -> impl IntoView {
    view! {
        <MetaTitle title="Alerts - Ultros" />
        <MetaDescription text="Manage your price alerts and notifications" />
        <div>
            <h1>"Alerts"</h1>
            "Todo: implement alerts"
        </div>
    }
}
