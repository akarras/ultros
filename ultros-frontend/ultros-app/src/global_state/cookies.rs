use cookie::{Cookie, CookieJar};

use leptos::*;
use log::info;

#[derive(Clone, Copy)]
pub struct Cookies {
    cookies: RwSignal<CookieJar>,
    // write_cookie_jar: WriteSignal<CookieJar>,
    // scope: Scope,
}

impl Cookies {
    pub fn new(cx: Scope) -> Self {
        let cookies = create_rw_signal(cx, get_cookies(cx).unwrap_or_default());
        create_effect(cx, move |_| {
            let cookie_jar = cookies();
            info!("updating cookies {cookie_jar:?}");
            set_cookies(cookie_jar);
        });
        Self { cookies }
    }

    pub fn get_cookie<C>(
        &self,
        cx: Scope,
        cookie_name: C,
    ) -> (
        Signal<Option<Cookie<'static>>>,
        SignalSetter<Option<Cookie<'static>>>,
    )
    where
        C: Copy + Clone + AsRef<str> + 'static,
    {
        // let cookie = &cookie_name;
        create_slice_non_copy(
            cx,
            self.cookies,
            move |cookies| {
                let cookie = cookie_name.as_ref();
                cookies.get(cookie).map(|c| c.clone().into_owned())
            },
            move |cookies, value| {
                if let Some(cookie) = value {
                    cookies.add(cookie.clone());
                }
            },
        )
    }
}

pub(crate) fn create_slice_non_copy<T, O>(
    cx: Scope,
    signal: RwSignal<T>,
    getter: impl Fn(&T) -> O + Clone + 'static,
    setter: impl Fn(&mut T, O) + Clone + 'static,
) -> (Signal<O>, SignalSetter<O>)
where
    O: PartialEq,
{
    let getter = create_memo(cx, move |_| signal.with(getter.clone()));
    let setter = move |value| signal.update(|x| setter(x, value));
    (getter.into(), setter.mapped_signal_setter(cx))
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
pub(crate) fn get_cookies(_cx: Scope) -> Option<CookieJar> {
    // use gloo::utils::document;
    use wasm_bindgen::JsCast;
    use web_sys::{window, HtmlDocument};
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
pub(crate) fn get_cookies(cx: Scope) -> Option<CookieJar> {
    use leptos_axum::RequestParts;
    let cookies = use_context::<RequestParts>(cx)?;
    let cookie = cookies.headers.get("Cookie")?;
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
