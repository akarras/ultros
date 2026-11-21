use leptos::{prelude::*, text_prop::TextProp};
use leptos_meta::*;

#[component]
pub fn MetaTitle(#[prop(into)] title: TextProp) -> impl IntoView {
    // SocialMetadata owns preview copy separately: route titles can include
    // account names or live market totals that must not enter crawler caches.
    view! { <Title text=resilient_title(title) /> }
}

/// Title text that survives being read after its page is disposed.
///
/// `leptos_meta` drives `document.title` from one app-wide render effect that
/// re-reads the top of its title stack whenever any `<Title>` mounts, updates
/// or unmounts. That effect outlives every page, and a page's title closure
/// can still be on the stack after the page's signals are gone: a route's
/// reactive view rebuilds once more while the router is already leaving it
/// (its params memo describes the next match before the old view is dropped),
/// pushes a fresh title whose closure reads page-owned signals, and the next
/// title change reads that closure back after disposal. Reading through a
/// memo with `try_get` turns that late read into an empty string instead of a
/// wasm panic (GlitchTip #7389).
pub fn resilient_title(title: TextProp) -> TextProp {
    let text = Memo::new(move |_| title.get().to_string());
    TextProp::from(move || text.try_get().unwrap_or_default())
}

/// Creates appropriate meta tags to indicate an image is present on the page
#[component]
pub fn MetaImage(#[prop(into)] url: TextProp, #[prop(into)] alt: TextProp) -> impl IntoView {
    view! {
        <Meta name="twitter:image" content=url.clone() />
        <Meta property="og:image" content=url />
        <Meta name="twitter:image:alt" content=alt.clone() />
        <Meta property="og:image:alt" content=alt />
        <Meta property="og:image:type" content="image/png" />
        <Meta property="og:image:width" content="1200" />
        <Meta property="og:image:height" content="630" />
    }
}

/// Creates appropriate meta tags for the description
#[component]
pub fn MetaDescription(#[prop(into)] text: TextProp) -> impl IntoView {
    view! {
        <Meta name="description" content=text />
    }
}

/// Tells search engines not to index this page. Use on routes that show
/// per-user data (alerts, retainers, settings, profile) or transient state
/// (invite-accept flows). These pages have no organic value and should
/// not be served as search results.
#[component]
pub fn MetaRobotsNoIndex() -> impl IntoView {
    view! { <Meta name="robots" content="noindex, follow" /> }
}

/// `(BCP-47 hreflang, Ultros `?lang=` code)` pairs for every shipped locale.
pub const HREFLANG_LOCALES: &[(&str, &str)] = &[
    ("en", "en"),
    ("ja", "ja"),
    ("de", "de"),
    ("fr", "fr"),
    ("ko", "ko"),
    ("zh-Hans", "cn"),
    ("zh-Hant", "tc"),
];

/// Build the localized alternate URL for a canonical base URL.
pub fn localized_alternate_url(base: &str, lang: &str) -> String {
    let sep = if base.contains('?') { '&' } else { '?' };
    format!("{base}{sep}lang={lang}")
}

/// Build the canonical URL for a tool route that optionally carries a `:world`
/// path segment (e.g. `/recipe-analyzer` or `/recipe-analyzer/Gilgamesh`).
pub fn canonical_tool_href(base_path: &str, world: Option<&str>) -> String {
    let base_path = base_path.trim_end_matches('/');
    match world.map(str::trim).filter(|w| !w.is_empty()) {
        Some(world) => {
            let encoded = leptos_router::location::Url::escape(world);
            format!("https://ultros.app{base_path}/{encoded}")
        }
        None => format!("https://ultros.app{base_path}"),
    }
}

/// Sets a canonical URL and 7-locale `hreflang` alternates for the current
/// indexable page. Use on routes that may be reachable via multiple URLs
/// (e.g. `/item/{world}/{id}` and `/item/{id}`) or that accept query params
/// that don't change the core entity (filters, sorts, `?lang=`).
///
/// `href` is a `TextProp` rather than a plain string because
/// `leptos_meta::Link`'s own `href` prop only accepts a static
/// `Oco<'static, str>` (no reactive closure support). Wrapping the `<Link>`s
/// in a reactive block here means a `move || ...` closure passed as `href`
/// re-registers the tags whenever its dependencies change, instead of only
/// ever emitting the value captured at first render.
#[component]
pub fn MetaCanonical(#[prop(into)] href: TextProp) -> impl IntoView {
    view! {
        {move || {
            let base = href.get().to_string();
            let alternates = HREFLANG_LOCALES
                .iter()
                .map(|&(bcp47, code)| {
                    let localized = localized_alternate_url(&base, code);
                    view! { <Link rel="alternate" hreflang=bcp47 href=localized /> }
                })
                .collect_view();
            view! {
                <Link rel="canonical" href=base.clone() />
                <Link rel="alternate" hreflang="x-default" href=base />
                {alternates}
            }
        }}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use leptos::reactive::owner::Owner;

    fn page_title(owner: &Owner, guarded: bool) -> TextProp {
        owner.with(|| {
            let name = RwSignal::new("Supplies".to_string());
            let title = TextProp::from(move || name.get());
            if guarded {
                resilient_title(title)
            } else {
                title
            }
        })
    }

    #[test]
    fn resilient_title_reads_empty_after_its_page_is_disposed() {
        let owner = Owner::new();
        let title = page_title(&owner, true);
        assert_eq!(title.get().to_string(), "Supplies");
        owner.cleanup();
        assert_eq!(title.get().to_string(), "");
    }

    /// The guard exists because a bare page title panics on the same read.
    #[test]
    #[should_panic(expected = "already been disposed")]
    fn bare_title_panics_after_its_page_is_disposed() {
        let owner = Owner::new();
        let title = page_title(&owner, false);
        owner.cleanup();
        let _ = title.get();
    }

    #[test]
    fn canonical_helpers_build_clean_urls_and_hreflang_alternates() {
        assert_eq!(
            canonical_tool_href("/recipe-analyzer", None),
            "https://ultros.app/recipe-analyzer"
        );
        assert_eq!(
            canonical_tool_href("/recipe-analyzer", Some("  ")),
            "https://ultros.app/recipe-analyzer"
        );
        assert_eq!(
            canonical_tool_href("/recipe-analyzer", Some("Gilgamesh")),
            "https://ultros.app/recipe-analyzer/Gilgamesh"
        );
        assert_eq!(
            localized_alternate_url("https://ultros.app/item/123", "ja"),
            "https://ultros.app/item/123?lang=ja"
        );
        assert_eq!(
            localized_alternate_url("https://ultros.app/item/123?foo=bar", "cn"),
            "https://ultros.app/item/123?foo=bar&lang=cn"
        );
        assert_eq!(HREFLANG_LOCALES.len(), 7);
    }
}
