use cookie::{time::Duration, Cookie, SameSite};
use leptos::{
    prelude::*,
    reactive::wrappers::write::{IntoSignalSetter, SignalSetter},
};
use ultros_api_types::{
    world::World,
    world_helper::{AnySelector, OwnedResult},
};

use super::{
    cookies::{get_now, Cookies},
    LocalWorldData,
};

const HOMEWORLD_COOKIE_NAME: &str = "HOME_WORLD";
const DEFAULT_PRICE_ZONE: &str = "PRICE_ZONE";

#[derive(Clone)]
pub struct GuessedRegion(pub String);

pub fn use_home_world() -> (Signal<Option<World>>, SignalSetter<Option<World>>) {
    let cookies = use_context::<Cookies>().unwrap();
    let (cookie, set_cookie) = cookies.get_cookie(HOMEWORLD_COOKIE_NAME);
    let world_1 = Some(use_context::<LocalWorldData>().unwrap().0.unwrap());
    let world_2 = world_1.clone();
    let world = Memo::new(move |_| {
        let cookie = cookie();
        world_1.as_ref().and_then(|w| {
            cookie.and_then(|cookie| {
                w.lookup_world_by_name(cookie.value())
                    .and_then(|w| w.as_world().map(|w| w.to_owned()))
            })
        })
    });
    let set_world = move |world: Option<World>| {
        // only set the world cookie once the worlds are populated
        if world_2.is_some() {
            let world = world.map(|w| {
                let mut cookie = Cookie::new(HOMEWORLD_COOKIE_NAME, w.name);
                cookie.set_same_site(SameSite::Lax);
                cookie.set_secure(Some(true));
                cookie.set_path("/");
                cookie.set_expires(get_now() + Duration::days(365));
                cookie
            });
            set_cookie(world);
        }
    };
    (world.into(), set_world.into_signal_setter())
}

pub fn result_to_selector_read(
    selector: Signal<Option<OwnedResult>>,
) -> Signal<Option<AnySelector>> {
    let signal = Memo::new(move |_| selector().map(|w| w.into()));
    signal.into()
}

pub fn selector_to_setter_signal(
    setter: SignalSetter<Option<OwnedResult>>,
) -> SignalSetter<Option<AnySelector>> {
    let signal = move |signal: Option<AnySelector>| {
        let world_data = use_context::<LocalWorldData>().unwrap().0.ok();
        if let Some(worlds) = signal.and_then(|signal| {
            world_data.and_then(|worlds| worlds.lookup_selector(signal).map(OwnedResult::from))
        }) {
            setter(Some(worlds))
        }
    };
    signal.into_signal_setter()
}

pub fn get_price_zone() -> (
    Signal<Option<OwnedResult>>,
    SignalSetter<Option<OwnedResult>>,
) {
    let cookies = use_context::<Cookies>().unwrap();
    let (cookie, set_cookie) = cookies.get_cookie(DEFAULT_PRICE_ZONE);
    let worlds = use_context::<LocalWorldData>().and_then(|worlds| worlds.0.ok());
    let guessed_region = use_context::<GuessedRegion>().map(|r| r.0);
    let world = Memo::new(move |_| {
        worlds.clone().and_then(|w| {
            cookie()
                .map(|cookie| cookie.value().to_string())
                .or_else(|| guessed_region.clone())
                .and_then(move |cookie| w.lookup_world_by_name(&cookie).map(|w| w.into()))
        })
    });

    let set_world = move |world: Option<OwnedResult>| {
        let worlds = use_context::<LocalWorldData>().unwrap().0;
        // only set the world cookie once the worlds are populated
        if worlds.ok().is_some() {
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
    (world.into(), set_world.into_signal_setter())
}

