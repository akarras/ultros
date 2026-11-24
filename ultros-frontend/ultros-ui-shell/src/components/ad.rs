use std::ops::Deref;

use crate::Cookies;
use crate::i18n::{t, t_string, use_i18n};
use leptos::{html::Ins, prelude::*};
use leptos_use::{UseMutationObserverOptions, use_mutation_observer_with_options};
use log::info;

pub const AD_CLIENT: &str = "ca-pub-8789160460804755";
/// Full AdSense loader URL shared between the SSR `<link rel="preload">` hint
/// and the post-hydration `<script>` injector so both always match.
pub const ADSENSE_LOADER_URL: &str =
    "https://pagead2.googlesyndication.com/pagead/js/adsbygoogle.js?client=ca-pub-8789160460804755";

/// Named AdSense slot IDs per placement.
///
/// All placements start on the existing unit ID so current inventory keeps
/// filling immediately; creating dedicated AdSense display units and updating
/// these constants unlocks per-placement RPM/viewability reporting and auction
/// optimization.
pub mod slots {
    pub const DEFAULT: &str = "1163555858";
    pub const DESKTOP_RAIL: &str = "1163555858";
    pub const HOME_BANNER: &str = "1163555858";
    pub const HOME_SIDEBAR: &str = "1163555858";
    pub const ITEM_BOTTOM: &str = "1163555858";
    pub const CURRENCY_INLINE: &str = "1163555858";
    pub const SETTINGS_PREVIEW: &str = "1163555858";
}

/// Client-side AdSense glue.
///
/// AdSense mutates the DOM it fills (it injects an iframe into the `<ins>`),
/// which breaks Leptos hydration if it runs against server-rendered HTML
/// before the client has claimed it — the ad renders with the SSR payload and
/// then vanishes the moment hydration finishes. So neither the loader
/// `<script>` nor the `adsbygoogle.push({})` call may appear in the SSR
/// output; both run from an effect after the `<ins>` has mounted on the
/// client.
#[cfg(feature = "hydrate")]
mod adsense {
    use super::ADSENSE_LOADER_URL;
    use leptos::prelude::*;
    use wasm_bindgen::{JsCast, JsValue, prelude::Closure};

    /// `id` of the loader `<script>` so remounting `Ad` components reuse it.
    /// AdSense wants the loader included once per page, in `<head>` —
    /// duplicating it per ad unit is flagged by Google's publisher audits.
    const LOADER_ID: &str = "ultros-adsbygoogle-loader";
    /// Set on the loader tag when it fails to load (ad blocker, offline) so
    /// ads mounted after the error can hide themselves immediately.
    const FAILED_ATTR: &str = "data-load-failed";

    /// Inject the AdSense loader `<script>` into `<head>`, exactly once per
    /// page. Marks `unfilled` when the loader can't load so the ad box hides.
    pub fn ensure_loader(unfilled: RwSignal<bool>) {
        let Some(document) = web_sys::window().and_then(|w| w.document()) else {
            return;
        };
        let script = if let Some(existing) = document.get_element_by_id(LOADER_ID) {
            existing
        } else {
            let Some(head) = document.head() else {
                return;
            };
            let Ok(script) = document.create_element("script") else {
                unfilled.try_set(true);
                return;
            };
            script.set_id(LOADER_ID);
            let _ = script.set_attribute("async", "");
            let _ = script.set_attribute("crossorigin", "anonymous");
            let _ = script.set_attribute("src", ADSENSE_LOADER_URL);
            let on_error = Closure::<dyn FnMut()>::new({
                let script = script.clone();
                move || {
                    let _ = script.set_attribute(FAILED_ATTR, "");
                    // try_set: the Ad that injected the loader may have been
                    // unmounted (and its signal disposed) by the time the
                    // error fires.
                    unfilled.try_set(true);
                }
            });
            let _ =
                script.add_event_listener_with_callback("error", on_error.as_ref().unchecked_ref());
            on_error.forget();
            if head.append_child(&script).is_err() {
                unfilled.try_set(true);
                return;
            }
            script
        };
        if script.has_attribute(FAILED_ATTR) {
            unfilled.try_set(true);
        }
    }

    /// `(adsbygoogle = window.adsbygoogle || []).push({})` — queue one fill
    /// for the most recently mounted unfilled `<ins class="adsbygoogle">`.
    /// Marks `unfilled` when AdSense rejects the request (it throws
    /// synchronously for e.g. zero-width slots).
    pub fn request_fill(unfilled: RwSignal<bool>) {
        let global = js_sys::global();
        let key = JsValue::from_str("adsbygoogle");
        let ads = match js_sys::Reflect::get(&global, &key) {
            Ok(v) if !v.is_undefined() && !v.is_null() => v,
            _ => {
                let queue = js_sys::Array::new();
                if js_sys::Reflect::set(&global, &key, &queue).is_err() {
                    unfilled.try_set(true);
                    return;
                }
                queue.into()
            }
        };
        let push = js_sys::Reflect::get(&ads, &JsValue::from_str("push"))
            .ok()
            .and_then(|f| f.dyn_into::<js_sys::Function>().ok());
        match push {
            Some(push) => {
                // Ignore push() errors. AdSense throws `TagError: … already
                // have ads` when its loader (requested with ?client= page-level
                // auto ads) has already filled this <ins> before our explicit
                // push runs — which means the ad IS present, so setting
                // `unfilled` here would HIDE a real, rendered ad. The mutation
                // observer on `data-ad-status` is the authoritative
                // filled/unfilled signal and already drives `unfilled`.
                let _ = push.call1(&ads, &js_sys::Object::new());
            }
            None => {
                unfilled.try_set(true);
            }
        }
    }
}

#[cfg(not(feature = "hydrate"))]
mod adsense {
    use leptos::prelude::*;

    /// SSR no-ops: the effect that calls these never runs on the server, but
    /// the component still has to compile without the wasm-only deps.
    pub fn ensure_loader(_unfilled: RwSignal<bool>) {}
    pub fn request_fill(_unfilled: RwSignal<bool>) {}
}

#[component]
pub fn Ad(
    #[prop(optional)] class: Option<&'static str>,
    #[prop(optional)] ad_slot: Option<&'static str>,
    #[prop(optional)] format: Option<&'static str>,
    #[prop(optional)] full_width_responsive: Option<bool>,
) -> impl IntoView {
    let i18n = use_i18n();
    let ad_class = class.unwrap_or("ad-banner-ins");
    let ad_slot = ad_slot.unwrap_or(slots::DEFAULT);
    let responsive_attr = full_width_responsive.map(|v| if v { "true" } else { "false" });
    let node = NodeRef::<Ins>::new();
    let cookies = use_context::<Cookies>().unwrap();
    let (hide_ads, _) = cookies.use_cookie_typed::<_, bool>("HIDE_ADS");
    let unfilled = RwSignal::new(false);
    let _mutation_observer = use_mutation_observer_with_options(
        node,
        move |mutations, _| {
            if let Some(_ad_fill_status) = mutations.into_iter().find(|record| {
                record
                    .attribute_name()
                    .map(|name| name == "data-ad-status")
                    .unwrap_or_default()
            }) {
                // just looking for data-ad-status="unfilled"
                let node = node.get_untracked().unwrap();
                if let Some(status) = node.deref().get_attribute("data-ad-status") {
                    info!("ad status {status}");
                    unfilled.set(status == "unfilled");
                }
            }
        },
        UseMutationObserverOptions::default().attributes(true),
    );
    // Runs client-side only, once the <ins> below has mounted — i.e. after
    // hydration on the first load, and on mount for every later client-side
    // navigation. Each mount gets a fresh <ins> and exactly one push({}),
    // which is the supported AdSense pattern for single-page apps.
    Effect::new(move |_| {
        if node.get().is_none() {
            return;
        }
        adsense::ensure_loader(unfilled);
        adsense::request_fill(unfilled);
    });
    let ads_visible = Signal::derive(move || !hide_ads.get().unwrap_or_default());
    view! {
        <Show when=ads_visible>
            <div class:hidden=unfilled class="ad">
                <div class="flex flex-col items-start gap-1 h-full">
                    <span class="text-[11px] uppercase tracking-wider px-1.5 py-0.5 rounded border border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,_var(--brand-ring)_10%,_transparent)] text-[color:var(--color-text-muted)] shrink max-w-fit leading-tight">
                        {t!(i18n, ads_label)}
                    </span>
                    <ins
                        class=["adsbygoogle block ", ad_class].concat()
                        data-ad-client=AD_CLIENT
                        data-ad-slot=ad_slot
                        data-ad-format=format
                        data-full-width-responsive=responsive_attr
                        node_ref=node
                    ></ins>
                </div>
            </div>
        </Show>
    }
    .into_any()
}

#[component]
pub fn DesktopAdRail() -> impl IntoView {
    let i18n = use_i18n();
    let cookies = use_context::<Cookies>().unwrap();
    let (hide_ads, _) = cookies.use_cookie_typed::<_, bool>("HIDE_ADS");
    let ads_visible = Signal::derive(move || !hide_ads.get().unwrap_or_default());

    view! {
        <Show when=ads_visible>
            <aside class="app-ad-rail" aria-label=t_string!(i18n, ad_aria_label)>
                <div class="ad-rail-slot sticky top-4">
                    // Sized in tailwind.css (.ad-rail-ins): 300x600, or
                    // 300x1050 on viewports tall enough to fit it.
                    <Ad class="ad-rail-ins" ad_slot=slots::DESKTOP_RAIL />
                </div>
            </aside>
        </Show>
    }
}
