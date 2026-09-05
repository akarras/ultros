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
        let nanos = (millis % 1000) * 1_000_000;
        let seconds = (millis / 1000) as i64;
        OffsetDateTime::from_unix_timestamp(seconds)
            .unwrap_or(OffsetDateTime::UNIX_EPOCH)
            .replace_nanosecond(nanos as u32)
            .unwrap_or(OffsetDateTime::UNIX_EPOCH)
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
                    remove_cookie(cookies, cookie_name.as_ref());
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
                set_shared_attributes(&mut cookie);
                cookie.set_expires(get_now() + Duration::days(365));
                cookie
            });
            set_cookie(cookie);
        };
        (typed_cookie, set_typed_cookie.into_signal_setter())
    }
}

/// The attributes every cookie written through [`Cookies`] carries, applied by
/// both the write and the removal so the two cannot drift apart.
///
/// A browser matches an incoming cookie against a stored one on
/// `(name, domain, path)` alone. `SameSite` and `Secure` are not part of that
/// key, but `Path` is — so a removal that leaves `Path` off is filed against
/// the *document's* default path instead of the cookie's. From `/settings`
/// that happens to be `/` and the delete lands; from `/item/2/Fire%20Shard` it
/// is `/item/2` and the site-wide cookie survives untouched. `SameSite=None`
/// is only accepted alongside `Secure`, so those two travel together.
fn set_shared_attributes(cookie: &mut Cookie<'static>) {
    cookie.set_same_site(SameSite::None);
    cookie.set_secure(Some(true));
    cookie.set_path("/");
}

/// The cookie that tells a browser to drop `cookie_name`.
///
/// Its rendered form is valid in both delivery paths — assigned to
/// `document.cookie` on the client, or sent as a `Set-Cookie` header from the
/// server — because the two share one grammar.
fn removal_cookie(cookie_name: &str) -> Cookie<'static> {
    let mut cookie = Cookie::new(cookie_name.to_string(), "");
    set_shared_attributes(&mut cookie);
    cookie.set_max_age(Duration::seconds(0));
    cookie.set_expires(get_now() - Duration::days(365));
    cookie
}

/// Queues the removal of `cookie_name` so that it actually reaches the browser.
///
/// [`CookieJar::remove`] only writes a removal into `delta()` when the name is
/// already in the jar's *original* set; for a cookie written earlier in this
/// same page load — which lives only in the delta — it takes its other branch
/// and silently drops the pending write, so nothing is ever handed to
/// `document.cookie`. That is the shipped ads bug: switching "hide ads" on and
/// then off again within one page load left `HIDE_ADS=true` on disk, and the
/// next reload brought the toggle back with no way to re-enable ads.
///
/// Seeding the original set forces the removal branch, and passing a
/// fully-attributed cookie means `make_removal` keeps the `Path`, `SameSite`
/// and `Secure` that [`set_shared_attributes`] put on the write.
fn remove_cookie(jar: &mut CookieJar, cookie_name: &str) {
    let removal = removal_cookie(cookie_name);
    jar.add_original(removal.clone());
    jar.remove(removal);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Renders a jar's pending writes the way `set_cookies` does: this is the
    /// exact string assigned to `document.cookie`, and the exact value a
    /// `Set-Cookie` header would carry.
    fn delta_headers(jar: &CookieJar) -> Vec<String> {
        jar.delta().map(|c| c.encoded().to_string()).collect()
    }

    /// A browser only deletes a cookie whose `(name, domain, path)` matches, so
    /// the removal has to repeat the write's `Path`. `SameSite=None` is
    /// rejected outright unless `Secure` rides along with it.
    #[test]
    fn the_removal_repeats_the_writes_attributes() {
        let removal = removal_cookie("HIDE_ADS");

        assert_eq!(removal.path(), Some("/"));
        assert_eq!(removal.same_site(), Some(SameSite::None));
        assert_eq!(removal.secure(), Some(true));
        assert_eq!(removal.max_age(), Some(Duration::seconds(0)));
        assert!(
            removal
                .expires_datetime()
                .is_some_and(|expiry| expiry < get_now()),
            "a removal needs an expiry in the past"
        );
    }

    /// The shipped ads bug. Toggling "hide ads" on and then off inside a single
    /// page load left `HIDE_ADS` only ever in the jar's *delta*, so
    /// `CookieJar::remove` took its non-original branch and simply dropped the
    /// pending write — `set_cookies` then had nothing to hand the browser, the
    /// cookie survived, and the next reload showed the toggle back on with no
    /// way to re-enable ads.
    #[test]
    fn a_cookie_written_this_page_load_is_still_deleted_in_the_browser() {
        let mut jar = CookieJar::new();
        let mut written = Cookie::new("HIDE_ADS", "true");
        set_shared_attributes(&mut written);
        jar.add(written);

        remove_cookie(&mut jar, "HIDE_ADS");

        assert!(
            jar.get("HIDE_ADS").is_none(),
            "the jar must read as unset so the toggle unchecks"
        );
        assert_eq!(
            delta_headers(&jar).len(),
            1,
            "the browser must be handed a removal, not left with the old value"
        );
    }

    /// The same delete on a cookie the browser sent us — the path a visitor
    /// takes after reloading with `HIDE_ADS=true` already stored.
    #[test]
    fn a_cookie_from_a_previous_visit_is_deleted_too() {
        let mut jar = CookieJar::new();
        jar.add_original(Cookie::new("HIDE_ADS", "true"));

        remove_cookie(&mut jar, "HIDE_ADS");

        assert!(jar.get("HIDE_ADS").is_none());
        assert_eq!(delta_headers(&jar).len(), 1);
    }

    /// Guards the serialized form itself, since that string is what both
    /// delivery paths carry: `document.cookie` on the client and `Set-Cookie`
    /// from the server. Before the fix this read `HIDE_ADS=; Max-Age=0;
    /// Expires=...` with no `Path`, so the browser filed it against the
    /// document's default path and left the site-wide cookie alone.
    #[test]
    fn the_removal_header_carries_every_matching_attribute() {
        let mut jar = CookieJar::new();
        jar.add_original(Cookie::new("HIDE_ADS", "true"));
        remove_cookie(&mut jar, "HIDE_ADS");

        let headers = delta_headers(&jar);
        let header = headers.first().expect("a removal must be queued");

        assert!(header.starts_with("HIDE_ADS="), "{header}");
        assert!(header.contains("; Path=/"), "{header}");
        assert!(header.contains("; SameSite=None"), "{header}");
        assert!(header.contains("; Secure"), "{header}");
        assert!(header.contains("; Max-Age=0"), "{header}");
        assert!(header.contains("; Expires="), "{header}");
    }

    /// Removing must not wedge the name: a visitor who turns ads back on, then
    /// off again, has to keep getting both writes.
    #[test]
    fn a_removed_cookie_can_be_written_and_removed_again() {
        let mut jar = CookieJar::new();
        jar.add_original(Cookie::new("HIDE_ADS", "true"));
        remove_cookie(&mut jar, "HIDE_ADS");

        let mut rewritten = Cookie::new("HIDE_ADS", "true");
        set_shared_attributes(&mut rewritten);
        jar.add(rewritten);
        assert_eq!(
            jar.get("HIDE_ADS").map(|c| c.value()),
            Some("true"),
            "re-enabling must take effect"
        );

        remove_cookie(&mut jar, "HIDE_ADS");
        assert!(jar.get("HIDE_ADS").is_none());
        assert_eq!(delta_headers(&jar).len(), 1);
    }
}
