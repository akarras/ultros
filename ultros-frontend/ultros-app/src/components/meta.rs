use leptos::{prelude::*, text_prop::TextProp};
use leptos_meta::*;

#[component]
pub fn MetaTitle(#[prop(into)] title: TextProp) -> impl IntoView {
    view! {
        <Title text=title.clone() />
        // OpenGraph keys are addressed by `property`, not `name`. Emitted as
        // `name="og:title"` this tag was invisible to every OG consumer
        // (Discord/Slack/Facebook unfurls), which fell back to <title>.
        <Meta property="og:title" content=title.clone() />
        <Meta name="twitter:title" content=title />
    }
}

/// Creates appropriate meta tags to indicate an image is present on the page
#[component]
pub fn MetaImage(#[prop(into)] url: TextProp) -> impl IntoView {
    view! {
        <Meta name="twitter:image" content=url.clone() />
        <Meta name="og:image" property="og:image" content=url />
    }
}

/// Creates appropriate meta tags for the description
#[component]
pub fn MetaDescription(#[prop(into)] text: TextProp) -> impl IntoView {
    view! {
        <Meta name="twitter:description" content=text.clone() />
        <Meta name="og:description" property="og:description" content=text.clone() />
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

/// Sets a canonical URL for the current page. Use on routes that may be
/// reachable via multiple URLs (e.g. /item/{world}/{id} and /item/{id})
/// or that accept query params that don't change page content.
///
/// `href` is a `TextProp` rather than a plain string because
/// `leptos_meta::Link`'s own `href` prop only accepts a static
/// `Oco<'static, str>` (no reactive closure support). Wrapping the `<Link>`
/// in a reactive block here means a `move || ...` closure passed as `href`
/// re-registers the tag (with the new value) whenever its dependencies
/// change, instead of only ever emitting the value captured at first render.
#[component]
pub fn MetaCanonical(#[prop(into)] href: TextProp) -> impl IntoView {
    view! { {move || view! { <Link rel="canonical" href=href.get() /> }} }
}
