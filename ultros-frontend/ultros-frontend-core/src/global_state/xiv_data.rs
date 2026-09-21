use leptos::prelude::*;

/// Reactive signal bumped whenever `xiv_gen_db`'s in-memory data is swapped to
/// a different locale. Components that display data from `xiv_gen_db::data()`
/// should call `tracked_data()` so they automatically re-render on swap.
#[derive(Copy, Clone)]
pub struct DataRevision(pub RwSignal<u32>);

pub fn provide_xiv_data_revision() {
    provide_context(DataRevision(RwSignal::new(0)));
}

/// The locale of the game-data pack the browser loaded *before* hydration
/// (`en`, `ja`, `cn`, ...). Provided by `ultros-client`; absent on the server,
/// where every pack is embedded and `tracked_data()` picks one per request.
/// Lets a synchronous locale switch during hydration tell whether the loaded
/// pack already matches (nothing to do) or must be swapped afterwards.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadedGameDataLocale(pub String);

/// Reactive equivalent of `xiv_gen_db::data()`. Registers the current reactive
/// scope as a subscriber of `DataRevision`, so the surrounding view re-renders
/// after a locale swap. Falls back to a plain read when no `DataRevision` is
/// in scope (SSR, tests, non-reactive callers).
pub fn tracked_data() -> &'static xiv_gen::Data {
    if let Some(rev) = use_context::<DataRevision>() {
        rev.0.track();
    }
    #[cfg(feature = "ssr")]
    {
        let locale = use_context::<leptos_i18n::I18nContext<crate::i18n::Locale>>()
            .map(|i18n| i18n.get_locale())
            .unwrap_or_default();
        xiv_gen_db::data_for(game_language(locale))
    }
    #[cfg(not(feature = "ssr"))]
    xiv_gen_db::data()
}

/// Fetches the rkyv-encoded data for `locale` from the server and swaps it into
/// `xiv_gen_db`. Caller is responsible for bumping `DataRevision` after this
/// resolves so subscribers re-render with the new data.
#[cfg(not(feature = "ssr"))]
pub async fn reload_xiv_data(locale: &str) -> anyhow::Result<()> {
    let response = gloo_net::http::Request::get(&xiv_gen_db::startup_url(locale))
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("fetch failed: {e}"))?;
    anyhow::ensure!(
        response.ok(),
        "game data request failed: {}",
        response.status()
    );
    let bytes = response
        .binary()
        .await
        .map_err(|e| anyhow::anyhow!("read body failed: {e}"))?;
    xiv_gen_db::try_init(&bytes)?;
    Ok(())
}

/// Whether an item with this id exists in the currently loaded xiv_gen data.
pub fn item_exists(id: i32) -> bool {
    tracked_data().items.contains_key(&xiv_gen::ItemId(id))
}

/// Detail resources serialize their SSR result into the response, so direct
/// navigation hydrates without a second request or a missing-NPC flash.
pub async fn npc_detail(
    locale: crate::i18n::Locale,
    id: i32,
) -> Result<Option<xiv_gen::ENpcResident>, String> {
    #[cfg(feature = "ssr")]
    {
        Ok(xiv_gen_db::data_for(game_language(locale))
            .e_npc_residents
            .get(&xiv_gen::ENpcResidentId(id))
            .cloned())
    }
    #[cfg(not(feature = "ssr"))]
    {
        fetch_detail(locale, "npc", id).await
    }
}

pub async fn item_description(
    locale: crate::i18n::Locale,
    id: i32,
) -> Result<Option<String>, String> {
    #[cfg(feature = "ssr")]
    {
        Ok(xiv_gen_db::data_for(game_language(locale))
            .items
            .get(&xiv_gen::ItemId(id))
            .map(|item| item.description.clone()))
    }
    #[cfg(not(feature = "ssr"))]
    {
        fetch_detail(locale, "description", id).await
    }
}

#[cfg(not(feature = "ssr"))]
async fn fetch_detail<T: serde::de::DeserializeOwned>(
    locale: crate::i18n::Locale,
    kind: &str,
    id: i32,
) -> Result<T, String> {
    send_wrapper::SendWrapper::new(async move {
        let lang = game_language(locale).to_path_part();
        let url = format!(
            "/static/game-detail/{}/{lang}/{kind}/{id}",
            xiv_gen_db::pack_version(lang)
        );
        let response = gloo_net::http::Request::get(&url)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !response.ok() {
            return Err(format!("Detail request failed ({})", response.status()));
        }
        response.json().await.map_err(|e| e.to_string())
    })
    .await
}

/// Parses a route path param as an item id, returning `None` if it doesn't
/// parse as an integer or doesn't name a real item — the two ways a garbage
/// `/item/<id>` URL currently falls through to a fake "item 0" page.
///
/// Id 0 is rejected explicitly. Row 0 of the game's Item sheet is a real row
/// (the unnamed "nothing here" placeholder), so `item_exists(0)` is `true` and
/// the page used to mount for `/item/<world>/0`. Every fetch in `crate::api`
/// short-circuits id 0 to `AppError::NoItem` without touching the network, and
/// the item page logs that failure with `tracing::error!` — which on the server
/// is reported to GlitchTip once per render of such a URL. Crawlers hit these
/// URLs steadily, so this produced a continuous stream of "Error getting value"
/// error events for what is really just a 404.
pub fn resolve_item_id(raw: Option<&str>) -> Option<i32> {
    let id: i32 = raw?.parse().ok()?;
    (id > 0 && item_exists(id)).then_some(id)
}

/// Matches the UI locale to the corresponding game-data pack.
pub fn game_language(locale: crate::i18n::Locale) -> xiv_gen::Language {
    use crate::i18n::Locale;
    use xiv_gen::Language;

    match locale {
        Locale::en => Language::En,
        Locale::ja => Language::Ja,
        Locale::de => Language::De,
        Locale::fr => Language::Fr,
        Locale::cn => Language::Cn,
        Locale::ko => Language::Ko,
        Locale::tc => Language::Tc,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_item_id_accepts_a_real_item() {
        let real_id = tracked_data().items.keys().next().expect("data loaded").0;
        assert_eq!(resolve_item_id(Some(&real_id.to_string())), Some(real_id));
    }

    #[test]
    fn resolve_item_id_rejects_unparseable_ids() {
        assert_eq!(resolve_item_id(Some("notanumber")), None);
    }

    #[test]
    fn resolve_item_id_rejects_nonexistent_ids() {
        assert_eq!(resolve_item_id(Some("999999999")), None);
    }

    #[test]
    fn resolve_item_id_rejects_missing_param() {
        assert_eq!(resolve_item_id(None), None);
    }

    #[test]
    fn item_zero_is_the_blank_placeholder_row() {
        // Row 0 of the game's Item sheet is a real row with an empty name — the
        // "nothing equipped" placeholder. It is present in the pack, so a bare
        // `contains_key` check treats /item/<world>/0 as a valid item.
        let blank = tracked_data().items.get(&xiv_gen::ItemId(0));
        assert!(blank.is_some(), "item 0 should be present in the pack");
        assert!(
            blank.unwrap().name.is_empty(),
            "item 0 should be the unnamed placeholder row"
        );
    }

    #[test]
    fn resolve_item_id_rejects_the_blank_item_zero() {
        // `/item/<world>/0` must render NotFound, not the item page: every
        // fetch in `crate::api` short-circuits id 0 to `AppError::NoItem`, and
        // the resulting `error!` is reported to GlitchTip once per SSR render.
        assert_eq!(resolve_item_id(Some("0")), None);
    }
}
