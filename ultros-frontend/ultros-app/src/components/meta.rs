use leptos::*;
use leptos_meta::*;

#[component]
pub fn MetaTitle(cx: Scope, #[prop(into)] title: TextProp) -> impl IntoView {
    view! {cx, <Title text=title/> }
}

/// Creates appropriate meta tags to indicate an image is present on the page
#[component]
pub fn MetaImage(cx: Scope, #[prop(into)] url: TextProp) -> impl IntoView {
    view! {cx,
        <Meta name="twitter:image" content=url.clone() />
        <Meta property="og:image" content=url />
    }
}

/// Creates appropriate meta tags for the description
#[component]
pub fn MetaDescription(cx: Scope, #[prop(into)] text: TextProp) -> impl IntoView {
    view! {cx,
        <Meta name="twitter:description" content=text.clone() />
        <Meta property="og:description" content=text.clone() />
        <Meta name="description" content=text />
    }
}
