use crate::components::meta::{MetaDescription, MetaTitle};
use leptos::prelude::*;

#[component]
pub fn PrivacyPolicy() -> impl IntoView {
    view! {
        <MetaTitle title="Privacy Policy - Ultros" />
        <MetaDescription text="Information about how Ultros collects and uses your data." />
        <div class="container">
            <h3 class="text-2xl">"Privacy Policy"</h3>
            "TODO: Add full privacy policy"
            <a href="/cookie-policy">"Cookie Policy"</a>
        </div>
    }
}
