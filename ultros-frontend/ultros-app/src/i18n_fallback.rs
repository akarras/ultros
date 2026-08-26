//! A non-panicking `use_i18n()`.
//!
//! `use_i18n()` is `I18nContext::from_context().expect("I18n context is
//! missing")`. That expectation holds for every component reached through a
//! normal render, because `AppInner` wraps the whole tree in
//! `<I18nContextProvider>` — but it does not hold for a component that ends up
//! rendering under an owner which never saw that `provide_context`.
//!
//! Prod hits exactly that. When a suspended SSR fragment's owner is disposed
//! before the fragment resolves, `ScopedFuture::new` falls back to
//! `Owner::current().unwrap_or_default()` and hands the children a *fresh,
//! empty* owner instead of failing loudly. The first context read in that
//! subtree then panics — and because a panic mid-response aborts the SSR
//! stream, one missing translation costs the whole page. GlitchTip #7168
//! (`skeleton.rs`, `SingleLineSkeleton`) and #7164 (`realtime_status.rs`,
//! `RealtimeStatus`) are that panic, still firing daily.
//!
//! [`use_i18n_or_default`] degrades instead: it reuses the provided context
//! when there is one, and otherwise initializes a standalone context. The
//! standalone one resolves its locale from the `Accept-Language` header and
//! the language cookie exactly like the real provider does, both of which are
//! read through `use_context` and so come back empty under a dead owner — so
//! in practice the fallback renders the default locale. Shipping a handful of
//! English strings in a loading placeholder is a far better outcome than a
//! truncated response.
//!
//! This mirrors the fix in [`crate::components::clipboard`] (#1187) for the
//! same failure mode on a different context.

use crate::i18n::Locale;
use leptos::prelude::use_context;
use leptos_i18n::I18nContext;
use leptos_i18n::context::init_i18n_context;

/// `use_i18n()` that falls back to a default-locale context instead of
/// panicking when no `I18nContext` was provided.
#[track_caller]
pub fn use_i18n_or_default() -> I18nContext<Locale> {
    use_context::<I18nContext<Locale>>().unwrap_or_else(init_i18n_context::<Locale>)
}

#[cfg(test)]
mod tests {
    use super::*;
    use leptos::prelude::*;

    /// Building an `I18nContext` creates an isomorphic `Effect`, which spawns
    /// onto the global executor. Tests have to stand one up themselves; it can
    /// only be initialized once per process, so a repeat call is expected to
    /// fail and is ignored.
    fn init_executor() {
        let _ = any_spawner::Executor::init_futures_executor();
    }

    /// Reproduces GlitchTip #7168 / #7164: a component reading i18n under an
    /// owner that never saw `<I18nContextProvider>`. `use_i18n()` panics with
    /// "I18n context is missing" here, which on the server kills the SSR
    /// response mid-stream.
    #[test]
    fn falls_back_to_the_default_locale_when_the_context_is_missing() {
        init_executor();
        let owner = Owner::new();
        owner.with(|| {
            assert_eq!(use_i18n_or_default().get_locale_untracked(), Locale::en);
        });
    }

    /// The other half of the reproduction: the accessor this module replaces
    /// really does panic in that situation. If this ever stops panicking,
    /// `use_i18n_or_default` has become unnecessary.
    #[test]
    #[should_panic(expected = "I18n context is missing")]
    fn plain_use_i18n_panics_when_the_context_is_missing() {
        init_executor();
        let owner = Owner::new();
        owner.with(|| {
            let _ = crate::i18n::use_i18n();
        });
    }

    #[test]
    fn reuses_the_provided_context() {
        init_executor();
        let owner = Owner::new();
        owner.with(|| {
            let provided = init_i18n_context::<Locale>();
            provided.set_locale(Locale::fr);
            provide_context(provided);
            assert_eq!(use_i18n_or_default().get_locale_untracked(), Locale::fr);
        });
    }
}
