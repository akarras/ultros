use crate::components::meta::{MetaDescription, MetaTitle};
use crate::i18n::*;
use leptos::prelude::*;

#[component]
pub fn PrivacyPolicy() -> impl IntoView {
    let i18n = use_i18n();
    view! {
        <div class="container mx-auto max-w-3xl space-y-4 p-4">
            <MetaTitle title=move || t_string!(i18n, privacy_policy_title).to_string() />
            <MetaDescription text=move || {
                t_string!(i18n, privacy_policy_meta_description).to_string()
            } />
            <h1 class="text-3xl font-bold">{t!(i18n, privacy_policy_title)}</h1>
            <p class="text-sm opacity-70">{t!(i18n, privacy_policy_last_updated)}</p>
            <p>{t!(i18n, privacy_policy_intro)}</p>

            <h2 class="text-2xl font-semibold pt-4">{t!(i18n, privacy_policy_auto_heading)}</h2>
            <p>{t!(i18n, privacy_policy_auto_body)}</p>

            <h2 class="text-2xl font-semibold pt-4">{t!(i18n, privacy_policy_cookies_heading)}</h2>
            <p>{t!(i18n, privacy_policy_cookies_body)}</p>
            <p>
                <a href="/cookie-policy">{t!(i18n, privacy_policy_cookie_link)}</a>
            </p>

            <h2 class="text-2xl font-semibold pt-4">{t!(i18n, privacy_policy_account_heading)}</h2>
            <p>{t!(i18n, privacy_policy_account_body)}</p>
            <p>{t!(i18n, privacy_policy_account_data_body)}</p>

            <h2 class="text-2xl font-semibold pt-4">
                {t!(i18n, privacy_policy_analytics_heading)}
            </h2>
            <p>{t!(i18n, privacy_policy_analytics_body)}</p>
            <p>
                <a href="https://policies.google.com/technologies/partner-sites" rel="noopener">
                    {t!(i18n, privacy_policy_google_data_link)}
                </a>
            </p>

            <h2 class="text-2xl font-semibold pt-4">{t!(i18n, privacy_policy_ads_heading)}</h2>
            <p>{t!(i18n, privacy_policy_ads_body)}</p>
            <p>
                <a href="https://adssettings.google.com" rel="noopener">
                    {t!(i18n, privacy_policy_ads_settings_link)}
                </a>
            </p>

            <h2 class="text-2xl font-semibold pt-4">{t!(i18n, privacy_policy_errors_heading)}</h2>
            <p>{t!(i18n, privacy_policy_errors_body)}</p>

            <h2 class="text-2xl font-semibold pt-4">{t!(i18n, privacy_policy_market_heading)}</h2>
            <p>{t!(i18n, privacy_policy_market_body)}</p>

            <h2 class="text-2xl font-semibold pt-4">{t!(i18n, privacy_policy_sharing_heading)}</h2>
            <p>{t!(i18n, privacy_policy_sharing_body)}</p>

            <h2 class="text-2xl font-semibold pt-4">
                {t!(i18n, privacy_policy_retention_heading)}
            </h2>
            <p>{t!(i18n, privacy_policy_retention_body)}</p>

            <h2 class="text-2xl font-semibold pt-4">{t!(i18n, privacy_policy_contact_heading)}</h2>
            <p>{t!(i18n, privacy_policy_contact_body)}</p>
            <p>
                <a href="https://discord.gg/pgdq9nGUP2" rel="noopener">
                    {t!(i18n, privacy_policy_contact_discord_link)}
                </a>
            </p>
        </div>
    }
}
