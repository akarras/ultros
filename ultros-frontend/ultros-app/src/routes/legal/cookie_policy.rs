use crate::components::meta::{MetaDescription, MetaTitle};
use crate::i18n::*;
use leptos::prelude::*;

#[component]
pub fn CookiePolicy() -> impl IntoView {
    let i18n = use_i18n();
    view! {
        <div class="main-content container mx-auto max-w-3xl space-y-4 p-2 sm:p-6">
            <MetaTitle title=move || t_string!(i18n, cookie_policy_title).to_string() />
            <MetaDescription text=move || {
                t_string!(i18n, cookie_policy_meta_description).to_string()
            } />
            <h1 class="text-3xl font-bold">{t!(i18n, cookie_policy_title)}</h1>
            <p>
                {t!(i18n, cookie_policy_intro)}
                <a href="https://ultros.app/cookie-policy">"https://ultros.app/cookie-policy"</a>
            </p>
            <h2 class="text-2xl font-semibold pt-4">
                {t!(i18n, cookie_policy_what_are_cookies)}
            </h2>
            <p>{t!(i18n, cookie_policy_what_are_cookies_body)}</p>
            <h2 class="text-2xl font-semibold pt-4">{t!(i18n, cookie_policy_how_we_use)}</h2>
            <p>{t!(i18n, cookie_policy_how_we_use_body)}</p>
            <h2 class="text-2xl font-semibold pt-4">{t!(i18n, cookie_policy_disabling)}</h2>
            <p>{t!(i18n, cookie_policy_disabling_body)}</p>
            <h2 class="text-2xl font-semibold pt-4">{t!(i18n, cookie_policy_cookies_we_set)}</h2>
            <ul class="list-disc pl-6 space-y-2">
                <li>
                    <p class="font-semibold">{t!(i18n, cookie_policy_login_cookies_title)}</p>
                    <p>{t!(i18n, cookie_policy_login_cookies_body)}</p>
                </li>
                <li>
                    <p class="font-semibold">{t!(i18n, cookie_policy_preference_cookies_title)}</p>
                    <p>{t!(i18n, cookie_policy_preference_cookies_body)}</p>
                </li>
            </ul>
            <h2 class="text-2xl font-semibold pt-4">{t!(i18n, cookie_policy_third_party)}</h2>
            <p>{t!(i18n, cookie_policy_third_party_body)}</p>
            <ul class="list-disc pl-6 space-y-2">
                <li>
                    <p>{t!(i18n, cookie_policy_analytics_body)}</p>
                </li>
                <li>
                    <p>{t!(i18n, cookie_policy_adsense_body)}</p>
                    <p>
                        <a href="https://policies.google.com/technologies/ads" rel="noopener">
                            {t!(i18n, cookie_policy_adsense_faq)}
                        </a>
                    </p>
                </li>
            </ul>

            <h2 class="text-2xl font-semibold pt-4">{t!(i18n, cookie_policy_more_info)}</h2>
            <p>{t!(i18n, cookie_policy_more_info_body)}</p>
            <p>{t!(i18n, cookie_policy_contact_intro)}</p>
            <ul class="list-disc pl-6">
                <li>
                    <a href="https://discord.gg/pgdq9nGUP2" rel="noopener">
                        {t!(i18n, cookie_policy_contact_discord)}
                    </a>
                </li>
                <li>{t!(i18n, cookie_policy_contact_email)}</li>
            </ul>
        </div>
    }
}
