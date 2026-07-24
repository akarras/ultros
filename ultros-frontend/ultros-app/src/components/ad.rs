use std::ops::Deref;

use crate::Cookies;
use crate::i18n::{t, t_string, use_i18n};
use leptos::{html::Ins, prelude::*};
use leptos_router::components::A;
use leptos_use::{UseMutationObserverOptions, use_mutation_observer_with_options};
use log::info;

const AD_CLIENT: &str = "ca-pub-8789160460804755";

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
    use super::AD_CLIENT;
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
            let _ = script.set_attribute(
                "src",
                &format!(
                    "https://pagead2.googlesyndication.com/pagead/js/adsbygoogle.js?client={AD_CLIENT}"
                ),
            );
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
                if push.call1(&ads, &js_sys::Object::new()).is_err() {
                    unfilled.try_set(true);
                }
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
pub fn Ad(#[prop(optional)] class: Option<&'static str>) -> impl IntoView {
    let i18n = use_i18n();
    let ad_class = class.unwrap_or("h-64");
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
                <div class="flex flex-col h-full">
                    <span class="text-sm px-2 py-0.5 rounded-md border border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,_var(--brand-ring)_14%,_transparent)] text-[color:var(--color-text-muted)] shrink max-w-fit">
                        "Advertisements"
                    </span>
                    // <!-- Ultros-Ad-Main -->
                    <ins
                        class=["adsbygoogle block ", ad_class].concat()

                        data-ad-client=AD_CLIENT
                        data-ad-slot="1163555858"
                        // data-adtest="on"
                        node_ref=node
                    ></ins>
                    <span class="text-neutral-500 italic text-sm">
                        "ads are optional. you may disable or enable them under "
                        <A href="/settings">{t!(i18n, ad_settings_link)}</A>
                    </span>
                </div>
            </div>
        </Show>
    }.into_any()
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
                <div class="ad-rail-slot sticky top-24">
                    <Ad class="h-[600px] w-full" />
                </div>
            </aside>
        </Show>
    }
}
