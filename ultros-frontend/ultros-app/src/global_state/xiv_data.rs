use leptos::prelude::*;

/// Reactive signal bumped whenever `xiv_gen_db`'s in-memory data is swapped to
/// a different locale. Components that display data from `xiv_gen_db::data()`
/// should call `tracked_data()` so they automatically re-render on swap.
#[derive(Copy, Clone)]
pub struct DataRevision(pub RwSignal<u32>);

pub fn provide_xiv_data_revision() {
    provide_context(DataRevision(RwSignal::new(0)));
}

/// Reactive equivalent of `xiv_gen_db::data()`. Registers the current reactive
/// scope as a subscriber of `DataRevision`, so the surrounding view re-renders
/// after a locale swap. Falls back to a plain read when no `DataRevision` is
/// in scope (SSR, tests, non-reactive callers).
pub fn tracked_data() -> &'static xiv_gen::Data {
    if let Some(rev) = use_context::<DataRevision>() {
        rev.0.track();
    }
    xiv_gen_db::data()
}

/// Fetches the rkyv-encoded data for `locale` from the server and swaps it into
/// `xiv_gen_db`. Caller is responsible for bumping `DataRevision` after this
/// resolves so subscribers re-render with the new data.
#[cfg(not(feature = "ssr"))]
pub async fn reload_xiv_data(locale: &str) -> anyhow::Result<()> {
    let version = xiv_gen::data_version();
    let url = format!("/static/data/{}/{}.rkyv", version, locale);
    let bytes = gloo_net::http::Request::get(&url)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("fetch failed: {e}"))?
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

/// Parses a route path param as an item id, returning `None` if it doesn't
/// parse as an integer or doesn't name a real item — the two ways a garbage
/// `/item/<id>` URL currently falls through to a fake "item 0" page.
pub fn resolve_item_id(raw: Option<&str>) -> Option<i32> {
    let id: i32 = raw?.parse().ok()?;
    item_exists(id).then_some(id)
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
}
