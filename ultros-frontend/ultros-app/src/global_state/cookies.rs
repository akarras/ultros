use cookie::SameSite;
use cookie::{Cookie, CookieJar};
use leptos::reactive::wrappers::write::{IntoSignalSetter, SignalSetter};
use std::{borrow::Cow, str::FromStr};
use time::{Duration, OffsetDateTime};

use leptos::prelude::*;
use log::error;

/// returns the current OffsetDateTime
pub fn get_now() -> OffsetDateTime {
    #[cfg(not(feature = "ssr"))]
    {
        let date = js_sys::Date::new_0();
        let millis = date.get_time() as i128;
        OffsetDateTime::from_unix_timestamp_nanos(millis * 1_000_000).unwrap()
    }
    #[cfg(feature = "ssr")]
    {
        OffsetDateTime::now_utc()
    }
}

#[derive(Clone, Copy)]
pub struct Cookies {
    cookies: RwSignal<CookieJar>,
    // write_cookie_jar: WriteSignal<CookieJar>,
    // scope: Scope,
}

impl Cookies {
    pub fn new() -> Self {
        let cookies = RwSignal::new(get_cookies().unwrap_or_default());
        Effect::new(move |_| {
            let cookie_jar = cookies();
            set_cookies(cookie_jar);
        });
        Self { cookies }
    }

    pub fn get_cookie<C>(
        &self,

        cookie_name: C,
    ) -> (
        Signal<Option<Cookie<'static>>>,
        SignalSetter<Option<Cookie<'static>>>,
    )
    where
        C: Copy + Clone + AsRef<str> + Send + Sync + 'static,
    {
        // let cookie = &cookie_name;
        create_slice_non_copy(
            self.cookies,
            move |cookies| {
                let cookie = cookie_name.as_ref();
                cookies.get(cookie).map(|c| c.clone().into_owned())
            },
            move |cookies, value| {
                if let Some(cookie) = value {
                    cookies.add(cookie.clone());
                } else {
                    cookies.remove(Cookie::from(cookie_name.as_ref().to_string()));
                }
            },
        )
    }
    pub fn use_cookie_typed<C, T>(
        &self,
        cookie_name: C,
    ) -> (Memo<Option<T>>, SignalSetter<Option<T>>)
    where
        C: Copy + Clone + AsRef<str> + Send + Sync + 'static,
        Cow<'static, str>: From<C>,
        T: FromStr + ToString + PartialEq + Send + Sync,
        <T as FromStr>::Err: std::fmt::Display,
    {
        let (cookie, set_cookie) = self.get_cookie(cookie_name);
        let typed_cookie = Memo::new(move |_| {
            let cookie = cookie();
            cookie.and_then(|c| {
                T::from_str(c.value())
                    .map_err(|e| {
                        error!(
                            "Error parsing value from typed cookie {} {}",
                            e,
                            std::any::type_name::<T>()
                        );
                    })
                    .ok()
            })
        });
        let set_typed_cookie = move |value: Option<T>| {
            let cookie = value.map(|cookie| cookie.to_string()).map(|value| {
                let mut cookie = Cookie::new(cookie_name, value);
                cookie.set_same_site(SameSite::None);
                cookie.set_secure(Some(true));
                cookie.set_path("/");
                cookie.set_expires(get_now() + Duration::days(365));
                cookie
            });
            set_cookie(cookie);
        };
        (typed_cookie, set_typed_cookie.into_signal_setter())
    }
}

pub(crate) fn create_slice_non_copy<T, O>(
    signal: RwSignal<T>,
    getter: impl Fn(&T) -> O + Clone + Send + Sync + 'static,
    setter: impl Fn(&mut T, O) + Clone + Send + Sync + 'static,
) -> (Signal<O>, SignalSetter<O>)
where
    O: PartialEq + Send + Sync,
    T: Send + Sync + 'static,
{
    let getter = Memo::new(move |_| signal.with(getter.clone()));
    let setter = move |value| signal.update(|x| setter(x, value));
    (getter.into(), setter.into_signal_setter())
}

#[cfg(not(feature = "ssr"))]
pub(crate) fn set_cookies(cookies: CookieJar) {
    use wasm_bindgen::JsCast;
    use web_sys::HtmlDocument;
    let document = document().dyn_into::<HtmlDocument>().unwrap();
    for cookie in cookies.delta() {
        document.set_cookie(&cookie.encoded().to_string()).unwrap();
    }
}
#[cfg(feature = "ssr")]
pub(crate) fn set_cookies(_cookies: CookieJar) {
    unimplemented!("Server can't set cookies");
}

#[cfg(not(feature = "ssr"))]
pub(crate) fn get_cookies() -> Option<CookieJar> {
    // use gloo::utils::document;
    use wasm_bindgen::JsCast;
    use web_sys::{HtmlDocument, window};
    let mut cookie_jar = CookieJar::new();
    let cookie = window()?
        .document()?
        .dyn_into::<HtmlDocument>()
        .ok()?
        .cookie()
        .ok()
        .unwrap_or_default();
    for cookie in Cookie::split_parse_encoded(cookie) {
        match cookie {
            Ok(o) => cookie_jar.add_original(o),
            Err(e) => log::error!("Error parsing cookie {e:?}"),
        }
    }
    Some(cookie_jar)
}

#[cfg(feature = "ssr")]
pub(crate) fn get_cookies() -> Option<CookieJar> {
    use axum::http::request::Parts;
    let request_parts = use_context::<Parts>().expect("Request parts not provided");
    let cookie = request_parts.headers.get("Cookie")?;
    let value = cookie.to_str().ok()?.to_string();
    let mut cookie_jar = CookieJar::new();
    for cookie in Cookie::split_parse_encoded(value) {
        match cookie {
            Ok(o) => cookie_jar.add_original(o),
            Err(e) => log::error!("Error parsing cookie {e:?}"),
        }
    }
    Some(cookie_jar)
}
