use cookie::{
    time::{Duration, OffsetDateTime},
    Cookie, SameSite,
};
use leptos::*;
use ultros_api_types::{
    world::World,
    world_helper::{AnySelector, OwnedResult},
};

use super::{cookies::Cookies, LocalWorldData};

const HOMEWORLD_COOKIE_NAME: &'static str = "HOME_WORLD";
const DEFAULT_PRICE_ZONE: &'static str = "PRICE_ZONE";

/// returns the current OffsetDateTime
fn get_now() -> OffsetDateTime {
    #[cfg(not(feature = "ssr"))]
    {
        js_sys::Date::new_0().into()
    }
    #[cfg(feature = "ssr")]
    {
        OffsetDateTime::now_utc()
    }
}

pub fn get_homeworld(cx: Scope) -> (Signal<Option<World>>, SignalSetter<Option<World>>) {
    let cookies = use_context::<Cookies>(cx).unwrap();
    let (cookie, set_cookie) = cookies.get_cookie(cx, HOMEWORLD_COOKIE_NAME);
    let worlds = use_context::<LocalWorldData>(cx).unwrap();
    let world = create_memo(cx, move |_| {
        let worlds = worlds.0.read(cx).map(|w| w.ok()).flatten();
        worlds.and_then(|w| {
            cookie().and_then(|cookie| {
                w.lookup_world_by_name(cookie.value())
                    .and_then(|w| w.as_world().map(|w| w.to_owned()))
            })
        })
    });
    let set_world = move |world: Option<World>| {
        // only set the world cookie once the worlds are populated
        if worlds.0.read(cx).map(|w| w.ok()).flatten().is_some() {
            let world = world.map(|w| {
                let mut cookie = Cookie::new(HOMEWORLD_COOKIE_NAME, w.name);
                cookie.set_same_site(SameSite::Strict);
                cookie.set_secure(Some(true));
                cookie.set_path("/");
                cookie.set_expires(get_now() + Duration::days(365));
                cookie
            });
            set_cookie(world);
        }
    };
    (world.into(), set_world.mapped_signal_setter(cx))
}

pub fn result_to_selector_read(
    cx: Scope,
    selector: Signal<Option<OwnedResult>>,
) -> Signal<Option<AnySelector>> {
    let signal = create_memo(cx, move |_| selector().map(|w| w.into()));
    signal.into()
}

pub fn selector_to_setter_signal(
    cx: Scope,
    setter: SignalSetter<Option<OwnedResult>>,
) -> SignalSetter<Option<AnySelector>> {
    let local_world_data = use_context::<LocalWorldData>(cx).unwrap();
    let signal = move |signal: Option<AnySelector>| {
        let world_data = local_world_data
            .0
            .read(cx)
            .map(|worlds| worlds.ok())
            .flatten();
        if let Some(worlds) = signal.and_then(|signal| {
            world_data
                .and_then(|worlds| worlds.lookup_selector(signal).map(|s| OwnedResult::from(s)))
        }) {
            setter(Some(worlds))
        }
    };
    signal.mapped_signal_setter(cx)
}

pub fn get_price_zone(
    cx: Scope,
) -> (
    Signal<Option<OwnedResult>>,
    SignalSetter<Option<OwnedResult>>,
) {
    let cookies = use_context::<Cookies>(cx).unwrap();
    let (cookie, set_cookie) = cookies.get_cookie(cx, DEFAULT_PRICE_ZONE);
    let worlds = use_context::<LocalWorldData>(cx).unwrap();
    let world = create_memo(cx, move |_| {
        let worlds = worlds.0.read(cx).map(|w| w.ok()).flatten();
        worlds.and_then(|w| {
            cookie()
                .and_then(move |cookie| w.lookup_world_by_name(cookie.value()).map(|w| w.into()))
        })
    });

    let set_world = move |world: Option<OwnedResult>| {
        // only set the world cookie once the worlds are populated
        if worlds.0.read(cx).map(|w| w.ok()).flatten().is_some() {
            let world = world.map(|w| {
                let mut cookie = Cookie::new(DEFAULT_PRICE_ZONE, w.get_name().to_string());
                cookie.set_same_site(SameSite::Strict);
                cookie.set_secure(Some(true));
                cookie.set_path("/");
                cookie.set_expires(get_now() + Duration::days(365));
                cookie
            });
            set_cookie(world);
        }
    };
    (world.into(), set_world.mapped_signal_setter(cx))
}
