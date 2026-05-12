use crate::i18n::*;
use leptos::prelude::*;

#[component]
pub fn CookiePolicy() -> impl IntoView {
    let i18n = use_i18n();
    view! {
        <h1>{t!(i18n, cookie_policy_title)}</h1>
        <p>
            {t!(i18n, cookie_policy_intro)}
            <a href="https://ultros.app/cookie-policy">"https://ultros.app/cookie-policy"</a>
        </p>
        <p>
            <strong>{t!(i18n, cookie_policy_what_are_cookies)}</strong>
        </p>
        <p>
            {t!(i18n, cookie_policy_what_are_cookies_body)}
        </p>
        <p>
            <strong>{t!(i18n, cookie_policy_how_we_use)}</strong>
        </p>
        <p>
            {t!(i18n, cookie_policy_how_we_use_body)}
        </p>
        <p>
            <strong>{t!(i18n, cookie_policy_disabling)}</strong>
        </p>
        <p>
            {t!(i18n, cookie_policy_disabling_body)}
            <a href="https://www.cookiepolicygenerator.com/cookie-policy-generator/">
                {t!(i18n, cookie_policy_generator_link)}
            </a>.
        </p>
        <p>
            <strong>{t!(i18n, cookie_policy_cookies_we_set)}</strong>
        </p>
        <ul>
            <li>
                <p>{t!(i18n, cookie_policy_account_cookies_title)}</p>
                <p>
                    {t!(i18n, cookie_policy_account_cookies_body)}
                </p>
            </li>
            <li>
                <p>{t!(i18n, cookie_policy_login_cookies_title)}</p>
                <p>
                    {t!(i18n, cookie_policy_login_cookies_body)}
                </p>
            </li>
        </ul>
        <p>
            <strong>{t!(i18n, cookie_policy_third_party)}</strong>
        </p>
        <p>
            {t!(i18n, cookie_policy_third_party_body)}
        </p>
        <ul>
            <li>
                <p>
                    {t!(i18n, cookie_policy_adsense_body)}
                </p>
                <p>
                    {t!(i18n, cookie_policy_adsense_faq)}
                </p>
            </li>
        </ul>

        <p>
            <strong>{t!(i18n, cookie_policy_more_info)}</strong>
        </p>
        <p>
            {t!(i18n, cookie_policy_more_info_body)}
        </p>
        <p>
            {t!(i18n, cookie_policy_more_info_link_intro)}
            <a href="https://www.cookiepolicygenerator.com/sample-cookies-policy/">
                {t!(i18n, cookie_policy_article_link)}
            </a> "."
        </p>
        <p>
            {t!(i18n, cookie_policy_contact_intro)}
        </p>
        <ul>
            <li>{t!(i18n, cookie_policy_contact_email)}</li>

        </ul>
    }
}
