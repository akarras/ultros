use leptos::*;
use leptos_meta::*;

#[component]
pub fn MetaTitle(#[prop(into)] title: TextProp) -> impl IntoView {
    view! {<Title text=title.clone() /><Meta name="og:title" content=title /> }
}

/// Creates appropriate meta tags to indicate an image is present on the page
#[component]
pub fn MetaImage(#[prop(into)] url: TextProp) -> impl IntoView {
    view! {
        <Meta name="twitter:image" content=url.clone() />
        <Meta property="og:image" content=url />
    }
}

/// Creates appropriate meta tags for the description
#[component]
pub fn MetaDescription(#[prop(into)] text: TextProp) -> impl IntoView {
    view! {
        <Meta name="twitter:description" content=text.clone() />
        <Meta property="og:description" content=text.clone() />
        <Meta name="description" content=text />
    }
}
