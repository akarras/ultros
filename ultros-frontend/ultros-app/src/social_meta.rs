//! A single evergreen social preview for every route. This sits above Routes,
//! so nested page titles and async market data cannot add competing OG tags.

use crate::components::meta::MetaImage;
use crate::i18n::Locale;
use crate::social_card::{
    SocialCardContent, SocialCardKind, og_locale, parse_locale, social_card_content,
};
use leptos::prelude::*;
use leptos_i18n::Locale as _;
use leptos_meta::Meta;
use leptos_router::hooks::use_location;
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

pub const SOCIAL_ORIGIN: &str = "https://ultros.app";
const URL_COMPONENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

pub fn social_image_url(locale: Locale, kind: &SocialCardKind, world: Option<&str>) -> String {
    format!("{SOCIAL_ORIGIN}{}", social_image_path(locale, kind, world))
}

pub(crate) fn social_image_path(
    locale: Locale,
    kind: &SocialCardKind,
    world: Option<&str>,
) -> String {
    let (kind_name, key) = kind.parts();
    let mut url = format!(
        "/social/v2/{}/{}/{}",
        locale.as_str(),
        kind_name,
        utf8_percent_encode(&key, URL_COMPONENT),
    );
    if matches!(kind, SocialCardKind::Item(_))
        && let Some(world) = world
    {
        url.push_str("?world=");
        url.push_str(&utf8_percent_encode(world, URL_COMPONENT).to_string());
    }
    url
}

/// Decode through the router's query parser so duplicate and escaped values
/// have the same interpretation during SSR and client-side navigation.
pub(crate) fn locale_from_query(query: &str) -> Option<Locale> {
    leptos_router::location::RequestUrl::new(&format!("/?{}", query.trim_start_matches('?')))
        .parse()
        .ok()?
        .search_params()
        .get_str("lang")
        .and_then(parse_locale)
}

/// Resolve an explicit share URL before AppInner constructs translated views.
/// Cookie and Accept-Language remain the provider's fallback for ordinary URLs.
pub(crate) fn request_locale() -> Option<Locale> {
    #[cfg(feature = "ssr")]
    {
        use_context::<axum::http::request::Parts>()
            .and_then(|parts| parts.uri.query().and_then(locale_from_query))
    }
    #[cfg(not(feature = "ssr"))]
    {
        web_sys::window()
            .and_then(|window| window.location().search().ok())
            .and_then(|search| locale_from_query(&search))
    }
}

/// World scope is part of an item's explicit path, never the visitor's cookie.
fn item_world(path: &str) -> Option<String> {
    let parts = path.trim_matches('/').split('/').collect::<Vec<_>>();
    match parts.as_slice() {
        ["item", world, _] => percent_encoding::percent_decode_str(world)
            .decode_utf8()
            .ok()
            .map(|world| world.into_owned()),
        _ => None,
    }
}

fn social_page_url(path: &str, locale: Locale, kind: &SocialCardKind) -> String {
    // A fallback card must not expose invite tokens, list ids or account paths.
    let path = if matches!(kind, SocialCardKind::Home) {
        "/"
    } else {
        path
    };
    format!("{SOCIAL_ORIGIN}{path}?lang={}", locale.as_str())
}

fn resolved_card(
    locale: Locale,
    kind: SocialCardKind,
    world: Option<&str>,
) -> (SocialCardKind, SocialCardContent) {
    match social_card_content(locale, &kind, world) {
        Some(content) => (kind, content),
        None => (
            SocialCardKind::Home,
            social_card_content(locale, &SocialCardKind::Home, None)
                .expect("every locale has a home card"),
        ),
    }
}

#[component]
pub(crate) fn SocialMetadata() -> impl IntoView {
    let location = use_location();
    // Unlocalized URLs have one reproducible crawler preview. The browser's
    // ShareLocale writes the selected UI language into URLs before sharing.
    let locale = Memo::new(move |_| {
        location
            .query
            .with(|query| query.get_str("lang").and_then(parse_locale))
            .unwrap_or(Locale::en)
    });
    let data_revision = use_context::<crate::global_state::xiv_data::DataRevision>();
    let card = Resource::new_blocking(
        move || {
            (
                locale.get(),
                location.pathname.get(),
                data_revision.map(|rev| rev.0.get()).unwrap_or_default(),
            )
        },
        |(locale, path, _)| async move {
            let kind = SocialCardKind::from_route(&path);
            let world = item_world(&path);
            let npc = if let SocialCardKind::Npc(id) = &kind {
                crate::global_state::xiv_data::npc_detail(locale, *id)
                    .await
                    .ok()
                    .flatten()
            } else {
                None
            };
            let content = crate::social_card::social_card_content_with_npc(
                locale,
                &kind,
                world.as_deref(),
                npc.as_ref(),
            );
            let (kind, content) = match content {
                Some(content) => (kind, content),
                None => resolved_card(locale, SocialCardKind::Home, None),
            };
            (
                social_image_url(locale, &kind, world.as_deref()),
                social_page_url(&path, locale, &kind),
                content,
            )
        },
    );
    let card = move || {
        card.get().unwrap_or_else(|| {
            let (kind, content) = resolved_card(locale.get(), SocialCardKind::Home, None);
            (
                social_image_url(locale.get(), &kind, None),
                social_page_url("/", locale.get(), &kind),
                content,
            )
        })
    };
    let title = move || format!("{} · Ultros", card().2.title);
    let description = move || card().2.description;

    view! {
        <Suspense>
        <Meta property="og:title" content=title />
        <Meta name="twitter:title" content=title />
        <Meta property="og:description" content=description />
        <Meta name="twitter:description" content=description />
        <Meta property="og:url" content=move || card().1 />
        <Meta property="og:locale" content=move || og_locale(locale.get()) />
        <MetaImage
            url=move || card().0
            alt=move || { let content = card().2; format!("Ultros. {}. {}", content.title, content.subtitle) }
        />
        {move || {
            [Locale::en, Locale::ja, Locale::de, Locale::fr, Locale::ko, Locale::cn, Locale::tc]
                .into_iter()
                .filter(|alternate| *alternate != locale.get())
                .map(|alternate| view! { <Meta property="og:locale:alternate" content=og_locale(alternate) /> })
                .collect_view()
        }}
        </Suspense>
    }
}

/// How `ShareLocale` rewrites the URL to carry `?lang=`.
#[cfg(any(not(feature = "ssr"), test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShareLocaleHistory {
    /// The address bar already shows this path (landing without `?lang=`,
    /// locale picked in place, back/forward): rewrite the current entry.
    Replace,
    /// The router has moved on but the address bar has not: its pushState is
    /// still pending, so this navigation takes it over as a push.
    Push,
}

/// For anchor clicks the router publishes the new URL signal first and only
/// calls pushState once the route's loaders finish. `ShareLocale`'s effect runs
/// inside that window, and a `replace` there would overwrite the history entry
/// the user is *leaving* (the router then skips its own push because the URL
/// signal moved on) — the back button would jump past the previous page. Push
/// while the address bar still shows the old path, and keep the link's
/// scroll-to-top; otherwise rewrite in place without scrolling.
#[cfg(any(not(feature = "ssr"), test))]
fn share_locale_history_mode(
    address_bar_path: Option<&str>,
    router_path: &str,
) -> ShareLocaleHistory {
    match address_bar_path {
        Some(current) if current != router_path => ShareLocaleHistory::Push,
        _ => ShareLocaleHistory::Replace,
    }
}

/// Keep browser URLs shareable after picking a language or navigating to a new
/// page. Explicit URLs also work without JavaScript through request_locale().
#[component]
pub(crate) fn ShareLocale() -> impl IntoView {
    #[cfg(not(feature = "ssr"))]
    {
        use leptos_router::{NavigateOptions, hooks::use_navigate};
        let i18n = crate::i18n::use_i18n();
        let location = use_location();
        let navigate = use_navigate();
        Effect::new(move |previous: Option<(String, Locale)>| {
            let path = location.pathname.get();
            let search = location.search.get();
            let locale = i18n.get_locale();
            let requested = locale_from_query(&search);
            let url_changed = previous
                .as_ref()
                .is_none_or(|(url, _)| *url != format!("{path}{search}"));
            let selected = if url_changed {
                requested.unwrap_or(locale)
            } else {
                locale
            };
            if selected != locale {
                i18n.set_locale(selected);
                crate::components::language_picker::reload_locale_data(selected);
            }
            if requested != Some(selected) {
                let mut query = location.query.get_untracked();
                query.remove("lang");
                query.insert("lang".to_string(), selected.as_str().to_string());
                let target = format!(
                    "{path}{}{}",
                    query.to_query_string(),
                    location.hash.get_untracked()
                );
                let address_bar_path = window().location().pathname().ok();
                let history = share_locale_history_mode(address_bar_path.as_deref(), &path);
                navigate(
                    &target,
                    NavigateOptions {
                        replace: history == ShareLocaleHistory::Replace,
                        scroll: history == ShareLocaleHistory::Push,
                        ..Default::default()
                    },
                );
            }
            (format!("{path}{search}"), selected)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn share_locale_pushes_only_while_the_router_push_is_pending() {
        // Anchor click: router already on /items, address bar still on /.
        assert_eq!(
            share_locale_history_mode(Some("/"), "/items"),
            ShareLocaleHistory::Push
        );
        // Landing without `?lang=`, locale picker, back/forward: same path.
        assert_eq!(
            share_locale_history_mode(Some("/items"), "/items"),
            ShareLocaleHistory::Replace
        );
        // Unknown address bar: never destroy history on a guess.
        assert_eq!(
            share_locale_history_mode(None, "/items"),
            ShareLocaleHistory::Replace
        );
    }

    #[test]
    fn explicit_query_locales_are_strict_and_decode_like_the_router() {
        assert_eq!(locale_from_query("?lang=ja"), Some(Locale::ja));
        assert_eq!(
            locale_from_query("lang=%74%63&sort=price"),
            Some(Locale::tc)
        );
        assert_eq!(locale_from_query("?lang=invalid"), None);
        assert_eq!(locale_from_query("?world=Gilgamesh"), None);
    }

    #[test]
    fn image_urls_have_explicit_locale_version_and_encoded_scope() {
        assert_eq!(
            social_image_url(
                Locale::ja,
                &SocialCardKind::Item(49318),
                Some("North-America")
            ),
            "https://ultros.app/social/v2/ja/item/49318?world=North-America"
        );
        assert_eq!(
            social_image_url(Locale::tc, &SocialCardKind::Home, Some("Gilgamesh")),
            "https://ultros.app/social/v2/tc/home/default"
        );
    }

    #[test]
    fn fallback_preview_urls_do_not_reveal_private_paths() {
        assert_eq!(
            social_page_url("/list/invite/secret", Locale::de, &SocialCardKind::Home),
            "https://ultros.app/?lang=de"
        );
        assert_eq!(
            item_world("/item/North%2DAmerica/49318"),
            Some("North-America".into())
        );
        assert_eq!(item_world("/item/49318"), None);
    }

    #[cfg(feature = "ssr")]
    #[test]
    fn unavailable_regional_items_resolve_to_the_localized_public_preview() {
        let english = xiv_gen_db::data_for(xiv_gen::Language::En);
        for locale in [Locale::cn, Locale::ko, Locale::tc] {
            let regional = xiv_gen_db::data_for(crate::social_card::game_language(locale));
            let missing = english.items.iter().find(|(id, item)| {
                id.0 > 0
                    && !item.name.trim().is_empty()
                    && regional
                        .items
                        .get(*id)
                        .is_none_or(|item| item.name.trim().is_empty())
            });
            // Also cover invalid IDs if a future release synchronizes packs.
            let id = missing.map_or(i32::MAX, |(id, _)| id.0);
            let (kind, content) =
                resolved_card(locale, SocialCardKind::Item(id), Some("Gilgamesh"));
            assert_eq!(kind, SocialCardKind::Home);
            assert_eq!(
                content,
                social_card_content(locale, &SocialCardKind::Home, None).unwrap()
            );
            assert!(social_image_url(locale, &kind, Some("Gilgamesh")).ends_with("/home/default"));
        }
    }
}
