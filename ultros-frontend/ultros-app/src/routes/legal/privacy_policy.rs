use crate::i18n::*;
use leptos::prelude::*;

#[component]
pub fn PrivacyPolicy() -> impl IntoView {
    let i18n = use_i18n();
    view! {
        <div class="container">
            <h3 class="text-2xl">{t!(i18n, privacy_policy_title)}</h3>
            {t!(i18n, privacy_policy_placeholder)}
            <a href="/cookie-policy">{t!(i18n, privacy_policy_cookie_link)}</a>
        </div>
    }
}
