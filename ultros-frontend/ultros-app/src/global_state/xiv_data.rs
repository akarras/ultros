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

/// Fetches the bincode for `locale` from the server and swaps it into
/// `xiv_gen_db`. Caller is responsible for bumping `DataRevision` after this
/// resolves so subscribers re-render with the new data.
#[cfg(not(feature = "ssr"))]
pub async fn reload_xiv_data(locale: &str) -> anyhow::Result<()> {
    let version = xiv_gen::data_version();
    let url = format!("/static/data/{}/{}.bincode", version, locale);
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
