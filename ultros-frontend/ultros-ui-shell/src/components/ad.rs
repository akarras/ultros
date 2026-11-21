use std::ops::Deref;

use crate::Cookies;
use crate::components::app_link::use_location_or_default;
use crate::i18n::{t, t_string, use_i18n};
use leptos::{html::Ins, prelude::*};
use leptos_use::{
    UseElementSizeReturn, UseMutationObserverOptions, use_element_size,
    use_mutation_observer_with_options,
};
use log::info;

pub const AD_CLIENT: &str = "ca-pub-8789160460804755";
/// Full AdSense loader URL shared between the SSR `<link rel="preload">` hint
/// and the post-hydration `<script>` injector so both always match.
pub const ADSENSE_LOADER_URL: &str =
    "https://pagead2.googlesyndication.com/pagead/js/adsbygoogle.js?client=ca-pub-8789160460804755";

/// Minimum time (ms) between SPA route-triggered ad remounts so rapid clicks
/// or redirects never spam AdSense with back-to-back requests.
pub const AD_REFRESH_COOLDOWN_MS: f64 = 30_000.0;

/// Named AdSense slot IDs per placement.
///
/// All placements start on the existing unit ID so current inventory keeps
/// filling immediately; creating dedicated AdSense display units and updating
/// these constants unlocks per-placement RPM/viewability reporting and auction
/// optimization.
pub mod slots {
    pub const DEFAULT: &str = "1163555858";
    pub const DESKTOP_RAIL: &str = "1163555858";
    pub const LAPTOP_ANCHOR: &str = "1163555858";
    pub const HOME_BANNER: &str = "1163555858";
    pub const HOME_SIDEBAR: &str = "1163555858";
    pub const ITEM_MID: &str = "1163555858";
    pub const ITEM_BOTTOM: &str = "1163555858";
    pub const CURRENCY_INLINE: &str = "1163555858";
    pub const CONTENT_INLINE: &str = "1163555858";
    pub const SETTINGS_PREVIEW: &str = "1163555858";
}

/// Bucket viewport widths to match Ultros's responsive ad breakpoints so GA4
/// telemetry can break down fill rate and opt-out rate per layout tier.
pub fn viewport_bucket(width_px: f64) -> &'static str {
    if !width_px.is_finite() || width_px < 1024.0 {
        "mobile"
    } else if width_px < 1536.0 {
        "laptop"
    } else {
        "wide"
    }
}

/// Pure decision helper for SPA route/entity ad refresh.
///
/// Returns `true` only when:
/// - a prior route key existed (`previous.is_some()`), so initial mount never
///   double-mounts;
/// - the key actually changed (query-param filter tweaks share the same key);
/// - the current slot has already been filled at least once (an un-requested
///   hidden or lazy slot has nothing stale to replace); and
/// - at least `cooldown_ms` have elapsed since the last fill.
pub fn should_refresh_ad(
    previous: Option<&str>,
    next: &str,
    has_filled: bool,
    elapsed_ms: f64,
    cooldown_ms: f64,
) -> bool {
    let Some(prev) = previous else {
        return false;
    };
    if !has_filled || next.is_empty() || prev == next {
        return false;
    }
    elapsed_ms.is_finite() && elapsed_ms >= cooldown_ms
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
    use super::{ADSENSE_LOADER_URL, viewport_bucket};
    use leptos::{html::Ins, prelude::*};
    use wasm_bindgen::{JsCast, JsValue, prelude::Closure};

    /// `id` of the loader `<script>` so remounting `Ad` components reuse it.
    /// AdSense wants the loader included once per page, in `<head>` —
    /// duplicating it per ad unit is flagged by Google's publisher audits.
    const LOADER_ID: &str = "ultros-adsbygoogle-loader";
    /// Set on the loader tag when it fails to load (ad blocker, offline) so
    /// ads mounted after the error can hide themselves immediately.
    const FAILED_ATTR: &str = "data-load-failed";
    const ANCHOR_DISMISSED_KEY: &str = "ultros_anchor_ad_dismissed";
    const HIDDEN_PREF_LOGGED_KEY: &str = "ultros_ad_hidden_pref_logged";

    pub fn now_ms() -> f64 {
        js_sys::Date::now()
    }

    pub fn current_viewport_bucket() -> &'static str {
        let width = web_sys::window()
            .and_then(|w| w.inner_width().ok())
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        viewport_bucket(width)
    }

    /// Best-effort GA4 event emission. Silently no-ops when `window.gtag` is
    /// absent (e.g. blocked or `/plugin` pages).
    pub fn track_ad_event(event_name: &str, placement: &str, ad_slot: &str) {
        let global = js_sys::global();
        let Ok(gtag) = js_sys::Reflect::get(&global, &JsValue::from_str("gtag")) else {
            return;
        };
        let Some(gtag) = gtag.dyn_ref::<js_sys::Function>() else {
            return;
        };
        let params = js_sys::Object::new();
        let _ = js_sys::Reflect::set(
            &params,
            &JsValue::from_str("ad_placement"),
            &JsValue::from_str(placement),
        );
        let _ = js_sys::Reflect::set(
            &params,
            &JsValue::from_str("ad_slot"),
            &JsValue::from_str(ad_slot),
        );
        let _ = js_sys::Reflect::set(
            &params,
            &JsValue::from_str("viewport_bucket"),
            &JsValue::from_str(current_viewport_bucket()),
        );
        let _ = gtag.call3(
            &JsValue::NULL,
            &JsValue::from_str("event"),
            &JsValue::from_str(event_name),
            &params,
        );
    }

    pub fn track_hidden_pref_once() {
        let Ok(Some(storage)) = web_sys::window()
            .map(|w| w.session_storage())
            .unwrap_or(Ok(None))
        else {
            return;
        };
        if storage
            .get_item(HIDDEN_PREF_LOGGED_KEY)
            .ok()
            .flatten()
            .is_some()
        {
            return;
        }
        let _ = storage.set_item(HIDDEN_PREF_LOGGED_KEY, "1");
        track_ad_event("ad_hidden_pref", "global", "none");
    }

    pub fn is_anchor_dismissed() -> bool {
        web_sys::window()
            .and_then(|w| w.session_storage().ok().flatten())
            .and_then(|s| s.get_item(ANCHOR_DISMISSED_KEY).ok().flatten())
            .is_some()
    }

    pub fn persist_anchor_dismissed() {
        if let Some(storage) = web_sys::window().and_then(|w| w.session_storage().ok().flatten()) {
            let _ = storage.set_item(ANCHOR_DISMISSED_KEY, "1");
        }
    }

    /// Returns `true` when the `<ins>` currently occupies non-zero layout box
    /// dimensions (i.e. neither it nor an ancestor is `display: none`).
    pub fn is_rendered_visible(node: &NodeRef<Ins>) -> bool {
        let Some(el) = node.get_untracked() else {
            return false;
        };
        let rect = el.get_bounding_client_rect();
        rect.width() > 0.0 && rect.height() > 0.0
    }

    /// Observe `node` with a 300px vertical prefetch margin and flip
    /// `in_viewport` once it nears the viewport, disconnecting immediately on
    /// first intersection and on component cleanup.
    pub fn observe_near_viewport(node: NodeRef<Ins>, in_viewport: RwSignal<bool>) {
        type ObserverHandle = (
            web_sys::IntersectionObserver,
            Closure<dyn FnMut(js_sys::Array, web_sys::IntersectionObserver)>,
        );
        let handle = StoredValue::new_local(None::<ObserverHandle>);

        Effect::new(move |_| {
            if in_viewport.get_untracked() {
                return;
            }
            let Some(el) = node.get() else {
                return;
            };
            handle.update_value(|slot| {
                if let Some((observer, _)) = slot.take() {
                    observer.disconnect();
                }
            });

            let cb = Closure::<dyn FnMut(js_sys::Array, web_sys::IntersectionObserver)>::new(
                move |entries: js_sys::Array, observer: web_sys::IntersectionObserver| {
                    let hit = entries.iter().any(|entry| {
                        entry
                            .dyn_into::<web_sys::IntersectionObserverEntry>()
                            .map(|entry| entry.is_intersecting())
                            .unwrap_or(false)
                    });
                    if hit {
                        let _ = in_viewport.try_set(true);
                        observer.disconnect();
                    }
                },
            );
            let opts = web_sys::IntersectionObserverInit::new();
            opts.set_root_margin("300px 0px");
            match web_sys::IntersectionObserver::new_with_options(
                cb.as_ref().unchecked_ref(),
                &opts,
            ) {
                Ok(observer) => {
                    let element: &web_sys::Element = el.unchecked_ref();
                    observer.observe(element);
                    handle.set_value(Some((observer, cb)));
                }
                Err(_) => {
                    // Fallback when IntersectionObserver is unavailable.
                    let _ = in_viewport.try_set(true);
                }
            }
        });

        on_cleanup(move || {
            handle.update_value(|slot| {
                if let Some((observer, _)) = slot.take() {
                    observer.disconnect();
                }
            });
        });
    }

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
                    track_ad_event("ad_blocked", "loader", "none");
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
    use leptos::{html::Ins, prelude::*};

    /// SSR no-ops: the effects that call these never run on the server, but
    /// the component still has to compile without the wasm-only deps.
    pub fn now_ms() -> f64 {
        0.0
    }
    pub fn track_ad_event(_event_name: &str, _placement: &str, _ad_slot: &str) {}
    pub fn track_hidden_pref_once() {}
    pub fn is_anchor_dismissed() -> bool {
        false
    }
    pub fn persist_anchor_dismissed() {}
    pub fn is_rendered_visible(_node: &NodeRef<Ins>) -> bool {
        false
    }
    pub fn observe_near_viewport(_node: NodeRef<Ins>, _in_viewport: RwSignal<bool>) {}
    pub fn ensure_loader(_unfilled: RwSignal<bool>) {}
    pub fn request_fill(_unfilled: RwSignal<bool>) {}
}

/// Single mounted `<ins class="adsbygoogle">` lifecycle. Remounted by `<Ad>`
/// whenever an SPA route/entity refresh triggers a new generation.
#[component]
fn AdUnit(
    ad_class: &'static str,
    ad_slot: &'static str,
    placement: &'static str,
    format: Option<&'static str>,
    responsive_attr: Option<&'static str>,
    lazy: bool,
    unfilled: RwSignal<bool>,
    on_requested: Callback<()>,
) -> impl IntoView {
    let node = NodeRef::<Ins>::new();
    let requested = RwSignal::new(false);
    let in_viewport = RwSignal::new(!lazy);
    let UseElementSizeReturn { width, height } = use_element_size(node);
    let has_layout_size = Signal::derive(move || width.get() > 0.0 && height.get() > 0.0);

    if lazy {
        adsense::observe_near_viewport(node, in_viewport);
    }

    let _mutation_observer = use_mutation_observer_with_options(
        node,
        move |mutations, _| {
            if mutations.into_iter().any(|record| {
                record
                    .attribute_name()
                    .map(|name| name == "data-ad-status")
                    .unwrap_or_default()
            }) && let Some(node) = node.get_untracked()
                && let Some(status) = node.deref().get_attribute("data-ad-status")
            {
                info!("ad status {status}");
                let is_unfilled = status == "unfilled";
                unfilled.set(is_unfilled);
                if status == "filled" {
                    adsense::track_ad_event("ad_filled", placement, ad_slot);
                } else if is_unfilled {
                    adsense::track_ad_event("ad_unfilled", placement, ad_slot);
                }
            }
        },
        UseMutationObserverOptions::default().attributes(true),
    );

    // Runs client-side only once `<ins>` has mounted, has non-zero rendered
    // dimensions (never firing into `display: none` responsive containers),
    // and — for `lazy=true` slots — has scrolled near the viewport. Each mount
    // triggers at most one `adsbygoogle.push({})`.
    Effect::new(move |_| {
        if requested.get_untracked() {
            return;
        }
        if node.get().is_none() || !has_layout_size.get() || !in_viewport.get() {
            return;
        }
        if !adsense::is_rendered_visible(&node) {
            return;
        }
        requested.set(true);
        on_requested.run(());
        adsense::ensure_loader(unfilled);
        adsense::request_fill(unfilled);
    });

    view! {
        <ins
            class=["adsbygoogle block ", ad_class].concat()
            data-ad-client=AD_CLIENT
            data-ad-slot=ad_slot
            data-ad-format=format
            data-full-width-responsive=responsive_attr
            node_ref=node
        ></ins>
    }
}

#[component]
pub fn Ad(
    #[prop(optional)] class: Option<&'static str>,
    #[prop(optional)] ad_slot: Option<&'static str>,
    #[prop(optional)] placement: Option<&'static str>,
    #[prop(optional)] format: Option<&'static str>,
    #[prop(optional)] full_width_responsive: Option<bool>,
    #[prop(optional)] lazy: bool,
    #[prop(optional, into)] refresh_key: Option<Signal<String>>,
) -> impl IntoView {
    let i18n = use_i18n();
    let ad_class = class.unwrap_or("ad-banner-ins");
    let ad_slot = ad_slot.unwrap_or(slots::DEFAULT);
    let placement = placement.unwrap_or("inline");
    let responsive_attr = full_width_responsive.map(|v| if v { "true" } else { "false" });
    let cookies = use_context::<Cookies>().unwrap();
    let (hide_ads, _) = cookies.use_cookie_typed::<_, bool>("HIDE_ADS");
    let unfilled = RwSignal::new(false);
    let generation = RwSignal::new(0u32);
    let last_fill_ms = RwSignal::new(None::<f64>);

    Effect::new(move |_| {
        if hide_ads.get().unwrap_or_default() {
            adsense::track_hidden_pref_once();
        }
    });

    let on_requested = Callback::new(move |()| {
        last_fill_ms.set(Some(adsense::now_ms()));
    });

    if let Some(refresh_key) = refresh_key {
        Effect::new(move |previous: Option<String>| {
            let next = refresh_key.get();
            let last_ms = last_fill_ms.get_untracked();
            let elapsed = last_ms.map(|ts| adsense::now_ms() - ts).unwrap_or_default();
            if should_refresh_ad(
                previous.as_deref(),
                &next,
                last_ms.is_some(),
                elapsed,
                AD_REFRESH_COOLDOWN_MS,
            ) {
                unfilled.set(false);
                last_fill_ms.set(None);
                generation.update(|g| *g = g.wrapping_add(1));
            }
            next
        });
    }

    let ads_visible = Signal::derive(move || !hide_ads.get().unwrap_or_default());
    view! {
        <Show when=ads_visible>
            <div class:hidden=unfilled class="ad">
                <div class="flex flex-col items-start gap-1 h-full">
                    <span class="text-[11px] uppercase tracking-wider px-1.5 py-0.5 rounded border border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,_var(--brand-ring)_10%,_transparent)] text-[color:var(--color-text-muted)] shrink max-w-fit leading-tight">
                        {t!(i18n, ads_label)}
                    </span>
                    <For
                        each=move || [generation.get()]
                        key=|gen_id| *gen_id
                        children=move |_| {
                            view! {
                                <AdUnit
                                    ad_class
                                    ad_slot
                                    placement
                                    format
                                    responsive_attr
                                    lazy
                                    unfilled
                                    on_requested
                                />
                            }
                        }
                    />
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
    let location = use_location_or_default();
    let route_key = Signal::derive(move || location.pathname.get());

    view! {
        <Show when=ads_visible>
            <aside class="app-ad-rail" aria-label=t_string!(i18n, ad_aria_label)>
                <div class="ad-rail-slot sticky top-4">
                    // Sized in tailwind.css (.ad-rail-ins): 300x600, or
                    // 300x1050 on viewports tall enough to fit it.
                    <Ad
                        class="ad-rail-ins"
                        ad_slot=slots::DESKTOP_RAIL
                        placement="desktop_rail"
                        refresh_key=route_key
                    />
                </div>
            </aside>
        </Show>
    }
}

/// Dismissible 728x90 sticky bottom leaderboard for medium desktop/laptop
/// viewports (1024px–1535px, or 1024px–1351px when the sidebar is collapsed),
/// where neither `<MobileBar />` nor `<DesktopAdRail />` is displayed.
///
/// Dismissal is remembered in `sessionStorage` and read inside a post-hydration
/// `Effect` so SSR and initial hydration trees always agree.
#[component]
pub fn LaptopBottomAnchorAd() -> impl IntoView {
    let i18n = use_i18n();
    let cookies = use_context::<Cookies>().unwrap();
    let (hide_ads, _) = cookies.use_cookie_typed::<_, bool>("HIDE_ADS");
    let dismissed = RwSignal::new(false);
    Effect::new(move |_| {
        if adsense::is_anchor_dismissed() {
            dismissed.set(true);
        }
    });
    let visible = Signal::derive(move || !hide_ads.get().unwrap_or_default() && !dismissed.get());
    let location = use_location_or_default();
    let route_key = Signal::derive(move || location.pathname.get());

    view! {
        <Show when=visible>
            <aside class="app-bottom-anchor-ad" aria-label=t_string!(i18n, ad_aria_label)>
                <div class="app-bottom-anchor-inner">
                    <Ad
                        class="ad-anchor-ins"
                        ad_slot=slots::LAPTOP_ANCHOR
                        placement="laptop_anchor"
                        refresh_key=route_key
                    />
                    <button
                        type="button"
                        class="app-bottom-anchor-dismiss"
                        aria-label=t_string!(i18n, close).to_string()
                        on:click=move |_| {
                            adsense::persist_anchor_dismissed();
                            dismissed.set(true);
                        }
                    >
                        "×"
                    </button>
                </div>
            </aside>
        </Show>
    }
}

#[cfg(test)]
mod tests {
    use super::{AD_REFRESH_COOLDOWN_MS, should_refresh_ad, viewport_bucket};

    #[test]
    fn viewport_buckets_match_ad_layout_breakpoints() {
        assert_eq!(viewport_bucket(375.0), "mobile");
        assert_eq!(viewport_bucket(1023.0), "mobile");
        assert_eq!(viewport_bucket(1024.0), "laptop");
        assert_eq!(viewport_bucket(1440.0), "laptop");
        assert_eq!(viewport_bucket(1536.0), "wide");
        assert_eq!(viewport_bucket(2560.0), "wide");
        assert_eq!(viewport_bucket(f64::NAN), "mobile");
    }

    #[test]
    fn refresh_skips_initial_mount_and_unchanged_routes() {
        assert!(!should_refresh_ad(
            None,
            "/item/123",
            true,
            60_000.0,
            AD_REFRESH_COOLDOWN_MS,
        ));
        assert!(!should_refresh_ad(
            Some("/item/123"),
            "/item/123",
            true,
            60_000.0,
            AD_REFRESH_COOLDOWN_MS,
        ));
    }

    #[test]
    fn refresh_waits_for_fill_and_cooldown() {
        // Hidden or not-yet-scrolled slot never filled: nothing stale to replace.
        assert!(!should_refresh_ad(
            Some("/item/123"),
            "/item/456",
            false,
            60_000.0,
            AD_REFRESH_COOLDOWN_MS,
        ));
        // Rapid click under cooldown does not spam ad requests.
        assert!(!should_refresh_ad(
            Some("/item/123"),
            "/item/456",
            true,
            10_000.0,
            AD_REFRESH_COOLDOWN_MS,
        ));
        // Route change after cooldown remounts cleanly.
        assert!(should_refresh_ad(
            Some("/item/123"),
            "/item/456",
            true,
            AD_REFRESH_COOLDOWN_MS,
            AD_REFRESH_COOLDOWN_MS,
        ));
    }
}
