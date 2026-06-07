use futures::future::try_join_all;
use itertools::Itertools;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::collections::HashMap;
use tracing::error;
use tracing::instrument;
use ultros_api_types::{
    ActiveListing, CurrentlyShownItem, ExtendedSaleHistory, FfxivCharacter,
    FfxivCharacterVerification,
    alert::{
        Alert, AlertEvent, CreateAlertRequest, CreateEndpointRequest,
        CreatePushSubscriptionRequest, DiscordWritableGuild, Endpoint, ResendResult,
        UpdateAlertRequest, UpdateEndpointRequest, VapidPublicKey,
    },
    cheapest_listings::{CheapestListings, CheapestListingsMap},
    item_stats::ItemStatsResponse,
    list::{
        CreateInvite, CreateList, List, ListActivity, ListInvite, ListItem, ListSharedGroup,
        ListSharedUser, ListWithPermission, ShareListGroup, ShareListUser,
    },
    market_heat::MarketHeatResponse,
    market_pulse::MarketPulseDto,
    recent_sales::RecentSales,
    resale_quality::{ResaleQualityRequest, ResaleQualityResponse},
    result::JsonErrorWrapper,
    retainer::{Retainer, RetainerListings},
    search::SearchResult,
    sparklines::{MoversResponse, SparklinesRequest, SparklinesResponse},
    trends::TrendsData,
    user::{OwnedRetainer, UserData, UserRetainerListings, UserRetainers, group::UserGroup},
};

use crate::error::{AppError, AppResult};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};

pub(crate) async fn search(query: &str) -> AppResult<Vec<SearchResult>> {
    let encoded_query = utf8_percent_encode(query, NON_ALPHANUMERIC).to_string();
    fetch_api(&format!("/api/v1/search?q={encoded_query}")).await
}

pub(crate) async fn get_listings(item_id: i32, world: &str) -> AppResult<CurrentlyShownItem> {
    if item_id == 0 {
        return Err(AppError::NoItem);
    }
    fetch_api(&format!("/api/v1/listings/{world}/{item_id}")).await
}

/// Pull a larger window of sales than the default listings endpoint returns.
/// Server caps `limit` at 5000.
pub(crate) async fn get_extended_sale_history(
    item_id: i32,
    world: &str,
    limit: u32,
) -> AppResult<ExtendedSaleHistory> {
    if item_id == 0 {
        return Err(AppError::NoItem);
    }
    fetch_api(&format!(
        "/api/v1/extended_history/{world}/{item_id}?limit={limit}"
    ))
    .await
}

/// This is okay because the client will send our login cookie.
///
/// Before falling back to the network, consult `BootstrapUser` — the SSR
/// handler resolves the user from the auth cookie on every page render, and
/// the client mirrors that into context on hydration from the bootstrap
/// script. When the context is present we never have to hit
/// `/api/v1/current_user`.
pub(crate) async fn get_login() -> AppResult<UserData> {
    use leptos::prelude::use_context;
    if let Some(crate::global_state::BootstrapUser(user)) =
        use_context::<crate::global_state::BootstrapUser>()
    {
        return user.ok_or(AppError::ApiError(
            ultros_api_types::result::ApiError::NotAuthenticated,
        ));
    }
    fetch_api("/api/v1/current_user").await
}

pub(crate) async fn delete_user() -> AppResult<()> {
    delete_api("/api/v1/current_user").await
}

/// Get analyzer data
pub(crate) async fn get_cheapest_listings(world_name: &str) -> AppResult<CheapestListings> {
    fetch_api(&format!("/api/v1/cheapest/{}", world_name)).await
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct ResaleStatsDto {
    pub(crate) profit: i32,
    pub(crate) item_id: i32,
    #[serde(default)]
    pub(crate) hq: bool,
    pub(crate) sold_within: String,
    pub(crate) return_on_investment: f32,
    pub(crate) world_id: i32,
    // Phase 2 deep-scan enrichment from the server. Defaulted so older
    // backends (or CH-degraded responses) still deserialize cleanly.
    #[serde(default)]
    pub(crate) confidence_band: ultros_api_types::trends::ConfidenceBand,
    #[serde(default)]
    pub(crate) vwap_30d: i32,
    #[serde(default)]
    pub(crate) sample_size_30d: u32,
    #[serde(default)]
    pub(crate) launder_suspicion: f32,
}

/// Query parameters for [`get_best_deals`]. All optional — server applies
/// sensible defaults (min_profit=None, filter_sale=None, limit=50,
/// show_suspicious=false).
#[derive(Debug, Clone, Default)]
pub(crate) struct BestDealsParams {
    pub min_profit: Option<i32>,
    /// "Day" | "Week" | "Month".
    pub filter_sale: Option<&'static str>,
    pub limit: Option<u32>,
    pub show_suspicious: Option<bool>,
}

pub(crate) async fn get_best_deals(
    world_name: &str,
    params: BestDealsParams,
) -> AppResult<Vec<ResaleStatsDto>> {
    let mut qs: Vec<String> = Vec::with_capacity(4);
    if let Some(p) = params.min_profit {
        qs.push(format!("min_profit={p}"));
    }
    if let Some(s) = params.filter_sale {
        qs.push(format!("filter_sale={s}"));
    }
    if let Some(l) = params.limit {
        qs.push(format!("limit={l}"));
    }
    if let Some(b) = params.show_suspicious {
        qs.push(format!("show_suspicious={}", if b { 1 } else { 0 }));
    }
    let query = if qs.is_empty() {
        String::new()
    } else {
        format!("?{}", qs.join("&"))
    };
    fetch_api(&format!("/api/v1/best_deals/{world_name}{query}")).await
}

#[allow(dead_code)]
pub(crate) async fn get_bulk_listings(
    world: &str,
    item_ids: impl Iterator<Item = i32>,
) -> AppResult<HashMap<i32, Vec<(ActiveListing, Option<Retainer>)>>> {
    if world.is_empty() {
        return Err(AppError::NoItem);
    }
    let ids = item_ids.format(",");
    fetch_api(&format!("/api/v1/bulkListings/{world}/{ids}")).await
}

/// Get most expensive
pub(crate) async fn get_recent_sales_for_world(region_name: &str) -> AppResult<RecentSales> {
    fetch_api(&format!("/api/v1/recentSales/{}", region_name)).await
}

/// Legacy v1 trends fetch — pre-bucketed `high_velocity / rising_price /
/// falling_price` lists. The new Trends page uses [`get_trends_v2`] and
/// reads `items` instead. Kept around for parity with the server
/// endpoint's no-query-arg behavior and any external consumer.
#[allow(dead_code)]
pub(crate) async fn get_trends(world_name: &str) -> AppResult<TrendsData> {
    fetch_api(&format!("/api/v1/trends/{world_name}")).await
}

/// Batch deep-scan enrichment for the Flip Finder. Returns per-row
/// confidence band, VWAP, sample size, and laundering suspicion for the
/// given `(item_id, hq)` tuples on `world_name`. `window_days` should be
/// 7, 30, or 90 (clamped server-side).
#[allow(dead_code)]
pub(crate) async fn get_resale_quality(
    world_name: &str,
    items: Vec<(i32, bool)>,
    window_days: u16,
) -> AppResult<ResaleQualityResponse> {
    let req = ResaleQualityRequest {
        items,
        window_days: Some(window_days),
    };
    post_api(&format!("/api/v1/resale_quality/{world_name}"), req).await
}

/// V2 trends fetch — flat `items` list backed by ClickHouse window
/// aggregates. `window_days` should be 7, 30, or 90 (other values are
/// clamped server-side to 30).
pub(crate) async fn get_trends_v2(
    world_name: &str,
    window_days: u16,
    show_suspicious: bool,
) -> AppResult<TrendsData> {
    fetch_api(&format!(
        "/api/v1/trends/{world_name}?window={window_days}&show_suspicious={}",
        if show_suspicious { 1 } else { 0 }
    ))
    .await
}

pub(crate) async fn get_market_pulse(world_name: &str) -> AppResult<MarketPulseDto> {
    fetch_api(&format!("/api/v1/market_pulse/{}", world_name)).await
}

pub(crate) async fn get_market_heat(world_name: &str) -> AppResult<MarketHeatResponse> {
    fetch_api(&format!("/api/v1/market_heat/{}", world_name)).await
}

pub(crate) async fn get_item_stats(world_name: &str, item_id: i32) -> AppResult<ItemStatsResponse> {
    fetch_api(&format!("/api/v1/item_stats/{}/{}", world_name, item_id)).await
}

/// `direction` is one of `rising` / `falling` / `volume`.
pub(crate) async fn get_movers(
    world_name: &str,
    direction: &str,
    limit: u32,
) -> AppResult<MoversResponse> {
    fetch_api(&format!(
        "/api/v1/movers/{}?direction={}&limit={}",
        world_name, direction, limit
    ))
    .await
}

#[allow(dead_code)]
pub(crate) async fn post_sparklines(
    world_name: &str,
    req: SparklinesRequest,
) -> AppResult<SparklinesResponse> {
    post_api(&format!("/api/v1/sparklines/{}", world_name), req).await
}

/// Returns a list of the logged in user's retainers
pub(crate) async fn get_retainers() -> AppResult<UserRetainers> {
    fetch_api("/api/v1/user/retainer").await
}

pub(crate) async fn get_retainer_listings(retainer_id: i32) -> AppResult<RetainerListings> {
    fetch_api(&format!("/api/v1/retainer/listings/{retainer_id}")).await
}

pub(crate) async fn get_user_retainer_listings() -> AppResult<UserRetainerListings> {
    fetch_api("/api/v1/user/retainer/listings").await
}

#[derive(Deserialize, Serialize, Clone)]
pub(crate) struct UndercutData {
    pub(crate) current: ActiveListing,
    pub(crate) cheapest: i32,
}

pub type Undercuts = Vec<(Option<FfxivCharacter>, Vec<(Retainer, Vec<UndercutData>)>)>;

pub(crate) async fn get_retainer_undercuts() -> AppResult<Undercuts> {
    // get our retainer data
    let retainer_data = get_user_retainer_listings().await?;
    // build a unique list of worlds and item ids so we can fetch additional info about them
    // optimized: use cheapest listings for each world & avoid looking up literally every retainer
    let worlds: Vec<i32> = retainer_data
        .retainers
        .iter()
        .flat_map(|(_, r)| r.iter().flat_map(|(_, l)| l.iter().map(|l| l.world_id)))
        .unique()
        .collect();
    let listings = try_join_all(worlds.into_iter().map(|world| async move {
        get_cheapest_listings(&world.to_string())
            .await
            // include the world id in the returned value
            .map(|listings| (world, listings))
    }))
    .await?;
    // flatten the listings down so it's more usable
    let listings_map: HashMap<i32, CheapestListingsMap> =
        listings
            .into_iter()
            .fold(HashMap::new(), |mut world_map, (world_id, item_data)| {
                if world_map.insert(world_id, item_data.into()).is_some() {
                    unreachable!("Should only be one world id from the set above.");
                }
                world_map
            });
    // Now remove every listing from the user retainer listings that is already the cheapest listing per world
    let retainer_data = retainer_data
        .retainers
        .into_iter()
        .map(|(c, retainers)| {
            (
                c,
                retainers
                    .into_iter()
                    .map(|(r, listings)| {
                        let new_listings = listings
                            .iter()
                            .filter_map(|listing| {
                                // use the world/item_id as keys to lookup the rest of the listings that match this retainer
                                listings_map
                                    .get(&listing.world_id)
                                    .and_then(|world_map| {
                                        let summary =
                                            world_map.find_matching_listings(listing.item_id);
                                        if listing.hq {
                                            summary.hq.map(|l| l.price)
                                        } else {
                                            summary.lowest_gil()
                                        }
                                    })
                                    .and_then(|cheapest| {
                                        (listing.price_per_unit > cheapest).then(|| UndercutData {
                                            current: listing.clone(),
                                            cheapest,
                                        })
                                    })
                            })
                            .collect();
                        (r, new_listings)
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();

    Ok(retainer_data)
}

/// Searches retainers based on their name
pub(crate) async fn search_retainers(name: String) -> AppResult<Vec<Retainer>> {
    if name.is_empty() {
        return Err(AppError::EmptyString);
    }
    fetch_api(&format!("/api/v1/retainer/search/{name}")).await
}

/// Claims the given retainer based on their id
pub(crate) async fn claim_retainer(retainer_id: i32) -> AppResult<()> {
    fetch_api(&format!("/api/v1/retainer/claim/{retainer_id}")).await
}

/// Unclaims the retainer based on the owned retainer id
pub(crate) async fn unclaim_retainer(owned_retainer_id: i32) -> AppResult<()> {
    fetch_api(&format!("/api/v1/retainer/unclaim/{owned_retainer_id}")).await
}

/// Gets the characters for this user
pub(crate) async fn get_characters() -> AppResult<Vec<FfxivCharacter>> {
    fetch_api("/api/v1/characters").await
}

/// Gets pending character verifications for this user
pub(crate) async fn get_character_verifications() -> AppResult<Vec<FfxivCharacterVerification>> {
    fetch_api("/api/v1/characters/verifications").await
}

pub(crate) async fn check_character_verification(character_id: i32) -> AppResult<bool> {
    fetch_api(&format!("/api/v1/characters/verify/{character_id}")).await
}

/// Starts to claim the given character
pub(crate) async fn claim_character(id: i32) -> AppResult<(i32, String)> {
    fetch_api(&format!("/api/v1/characters/claim/{id}")).await
}

pub(crate) async fn unclaim_character(id: i32) -> AppResult<(i32, String)> {
    fetch_api(&format!("/api/v1/characters/unclaim/{id}")).await
}

/// Searches for the given character with the given lodestone ID.
pub(crate) async fn search_characters(character: String) -> AppResult<Vec<FfxivCharacter>> {
    fetch_api(&format!("/api/v1/characters/search/{character}")).await
}

pub(crate) async fn get_lists_with_permissions() -> AppResult<Vec<ListWithPermission>> {
    fetch_api("/api/v1/list").await
}

pub(crate) async fn get_lists() -> AppResult<Vec<List>> {
    Ok(get_lists_with_permissions()
        .await?
        .into_iter()
        .map(|entry| entry.list)
        .collect())
}

pub(crate) async fn get_list_items_with_listings(
    list_id: i32,
) -> AppResult<(ListWithPermission, Vec<(ListItem, Vec<ActiveListing>)>)> {
    if list_id == 0 {
        return Err(AppError::BadList);
    }
    fetch_api(&format!("/api/v1/list/{list_id}/listings")).await
}

pub(crate) async fn get_list_activity(list_id: i32) -> AppResult<Vec<ListActivity>> {
    if list_id == 0 {
        return Err(AppError::BadList);
    }
    fetch_api(&format!("/api/v1/list/{list_id}/activity?limit=50")).await
}

pub(crate) async fn delete_list(list_id: i32) -> AppResult<()> {
    delete_api(&format!("/api/v1/list/{list_id}/delete")).await
}

pub(crate) async fn leave_list(list_id: i32, self_user_id: u64) -> AppResult<()> {
    delete_api(&format!("/api/v1/list/{list_id}/share/user/{self_user_id}")).await
}

pub(crate) async fn create_list(list: CreateList) -> AppResult<()> {
    post_api("/api/v1/list/create", list).await
}

pub(crate) async fn edit_list(list: List) -> AppResult<()> {
    post_api("/api/v1/list/edit", list).await
}

pub(crate) async fn bulk_add_item_to_list(
    list_id: i32,
    list_items: Vec<ListItem>,
) -> AppResult<()> {
    post_api(&format!("/api/v1/list/{list_id}/add/items"), list_items).await
}

pub(crate) async fn add_item_to_list(list_id: i32, list_item: ListItem) -> AppResult<()> {
    post_api(&format!("/api/v1/list/{list_id}/add/item"), list_item).await
}

pub(crate) async fn edit_list_item(list_item: ListItem) -> AppResult<()> {
    post_api("/api/v1/list/item/edit", list_item).await
}

pub(crate) async fn delete_list_item(list_id: i32) -> AppResult<()> {
    delete_api(&format!("/api/v1/list/item/{list_id}/delete")).await
}

pub(crate) async fn delete_list_items(list_items: Vec<i32>) -> AppResult<()> {
    post_api("/api/v1/list/item/delete", list_items).await
}

pub(crate) async fn get_groups() -> AppResult<Vec<UserGroup>> {
    fetch_api("/api/v1/group").await
}

pub(crate) async fn get_list_shares(
    list_id: i32,
) -> AppResult<(Vec<ListSharedUser>, Vec<ListSharedGroup>)> {
    fetch_api(&format!("/api/v1/list/{list_id}/shares")).await
}

pub(crate) async fn share_list_with_user(list_id: i32, share: ShareListUser) -> AppResult<()> {
    post_api(&format!("/api/v1/list/{list_id}/share/user"), share).await
}

pub(crate) async fn share_list_with_group(list_id: i32, share: ShareListGroup) -> AppResult<()> {
    post_api(&format!("/api/v1/list/{list_id}/share/group"), share).await
}

pub(crate) async fn unshare_list_from_user(list_id: i32, user_id: i64) -> AppResult<()> {
    delete_api(&format!("/api/v1/list/{list_id}/share/user/{user_id}")).await
}

pub(crate) async fn unshare_list_from_group(list_id: i32, group_id: i32) -> AppResult<()> {
    delete_api(&format!("/api/v1/list/{list_id}/share/group/{group_id}")).await
}

pub(crate) async fn get_list_invites(list_id: i32) -> AppResult<Vec<ListInvite>> {
    fetch_api(&format!("/api/v1/list/{list_id}/invites")).await
}

pub(crate) async fn create_list_invite(
    list_id: i32,
    invite: CreateInvite,
) -> AppResult<ListInvite> {
    post_api(&format!("/api/v1/list/{list_id}/invite/create"), invite).await
}

pub(crate) async fn use_list_invite(invite_id: String) -> AppResult<i32> {
    post_api(&format!("/api/v1/invite/{invite_id}/use"), ()).await
}

pub(crate) async fn delete_list_invite(invite_id: String) -> AppResult<()> {
    delete_api(&format!("/api/v1/invite/{invite_id}")).await
}

pub(crate) async fn update_retainer_order(retainers: Vec<OwnedRetainer>) -> AppResult<()> {
    post_api("/api/v1/retainer/reorder", retainers).await
}

pub(crate) async fn get_alerts() -> AppResult<Vec<Alert>> {
    fetch_api("/api/v1/alerts").await
}

pub(crate) async fn create_alert(req: CreateAlertRequest) -> AppResult<Alert> {
    post_api("/api/v1/alerts", req).await
}

pub(crate) async fn patch_alert(id: i32, req: UpdateAlertRequest) -> AppResult<()> {
    patch_api(&format!("/api/v1/alerts/{id}"), req).await
}

pub(crate) async fn delete_alert(id: i32) -> AppResult<()> {
    delete_api(&format!("/api/v1/alerts/{id}")).await
}

pub(crate) async fn get_alert_events() -> AppResult<Vec<AlertEvent>> {
    fetch_api("/api/v1/alerts/events").await
}

pub(crate) async fn list_endpoints() -> AppResult<Vec<Endpoint>> {
    fetch_api("/api/v1/endpoints").await
}

pub(crate) async fn list_discord_writable_guilds() -> AppResult<Vec<DiscordWritableGuild>> {
    fetch_api("/api/v1/endpoints/discord-guilds").await
}

pub(crate) async fn create_endpoint(req: CreateEndpointRequest) -> AppResult<Endpoint> {
    post_api("/api/v1/endpoints", req).await
}

#[allow(dead_code)]
pub(crate) async fn update_endpoint(id: i32, req: UpdateEndpointRequest) -> AppResult<()> {
    patch_api(&format!("/api/v1/endpoints/{id}"), req).await
}

pub(crate) async fn delete_endpoint(id: i32) -> AppResult<()> {
    delete_api(&format!("/api/v1/endpoints/{id}")).await
}

pub(crate) async fn test_endpoint(id: i32) -> AppResult<ResendResult> {
    post_api(&format!("/api/v1/endpoints/{id}/test"), ()).await
}

pub(crate) async fn resend_alert_event(event_id: i64) -> AppResult<ResendResult> {
    post_api(&format!("/api/v1/alerts/events/{event_id}/resend"), ()).await
}

/// Fetch the server's VAPID public key. Used by the browser to call
/// `pushManager.subscribe({applicationServerKey})`.
///
/// SSR builds never invoke this — the browser-side subscribe flow lives behind
/// `cfg(all(feature = "hydrate", target_arch = "wasm32"))` — so this is "dead"
/// on the server. The allow is targeted, not a `#[allow]` smell.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) async fn get_vapid_public_key() -> AppResult<VapidPublicKey> {
    fetch_api("/api/v1/push/vapid-public-key").await
}

/// Persist the browser's PushSubscription on the server and create a matching
/// notification endpoint of method=WebPush. SSR-dead, same reasoning as
/// [`get_vapid_public_key`].
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) async fn create_push_subscription(
    req: CreatePushSubscriptionRequest,
) -> AppResult<Endpoint> {
    post_api("/api/v1/push/subscribe", req).await
}

/// Return the T, or try and return an AppError
#[instrument]
fn deserialize<T>(json: &str) -> AppResult<T>
where
    T: DeserializeOwned,
{
    let data = serde_json::from_str(json);
    match data {
        Ok(d) => Ok(d),
        // try to deserialize as SystemError, if that fails then return this error
        Err(e) => {
            if let Ok(d) = serde_json::from_str::<JsonErrorWrapper>(json) {
                match d {
                    JsonErrorWrapper::ApiError(api) => Err(api.into()),
                }
            } else if let Ok(d) = serde_json::from_str::<JsonErrorWrapper>(json) {
                Err(match d {
                    JsonErrorWrapper::ApiError(api) => AppError::ApiError(api),
                })
            } else {
                Err(AppError::Json(e.to_string()))
            }
        }
    }
}

/// Classify an internal-API response (HTTP status + body) into our
/// [`AppResult`]. Split out of the SSR fetch helpers so the status check can't
/// be skipped again — and so it's unit-testable without a live server.
///
/// * **Success status** — the body is the JSON-encoded `T`. (A handful of
///   endpoints answer `200` with a [`JsonErrorWrapper`] instead; [`deserialize`]
///   already unwraps those into the matching [`AppError`].)
/// * **Non-success status** — the body is *never* a `T`. It's either the API's
///   structured [`JsonErrorWrapper`] or a plain-text message — most commonly the
///   analyzer's `503 "Still warming up with data, unable to serve requests."`
///   during its post-deploy warm-up. Feeding that body to `serde_json` produces
///   a misleading `expected value at line 1 column 1` error reported at error
///   level — the noise behind GlitchTip issue 2218. We map the status
///   explicitly instead, mirroring the server side (`ultros/src/web/error.rs`).
#[cfg(feature = "ssr")]
fn parse_internal_api_response<T>(status: reqwest::StatusCode, body: &str) -> AppResult<T>
where
    T: DeserializeOwned,
{
    if status.is_success() {
        return deserialize(body);
    }
    // Preserve the API's structured error when it sent one...
    if let Ok(JsonErrorWrapper::ApiError(api)) = serde_json::from_str::<JsonErrorWrapper>(body) {
        return Err(AppError::ApiError(api));
    }
    // ...otherwise fall back to the plain-text body (e.g. the analyzer warm-up
    // message). This is an error *response*, not malformed JSON.
    Err(AppError::ApiError(
        ultros_api_types::result::ApiError::Message(body.trim().to_string()),
    ))
}

#[cfg(not(feature = "ssr"))]
#[instrument(skip())]
pub(crate) async fn delete_api<T>(path: &str) -> AppResult<T>
where
    T: DeserializeOwned,
{
    use leptos::task::spawn_local;
    let (tx, rx) = flume::unbounded();
    let path = path.to_string();
    spawn_local(async move {
        let inner_impl = async move || -> AppResult<String> {
            let json: String = gloo_net::http::Request::delete(&path)
                .credentials(web_sys::RequestCredentials::Include)
                .send()
                .await
                .inspect_err(|e| {
                    error!("{}", e);
                })?
                .text()
                .await?;
            Ok(json)
        };
        let result = inner_impl().await;
        tx.send(result).unwrap();
    });
    let json = rx
        .into_recv_async()
        .await
        .expect("The channel to just work")?;
    deserialize(&json)
}

#[cfg(feature = "ssr")]
#[instrument(skip())]
pub(crate) async fn delete_api<T>(path: &str) -> AppResult<T>
where
    T: DeserializeOwned,
{
    use axum::http::request::Parts;
    use leptos::prelude::use_context;
    // use the original headers of the scope
    // add the hostname when using the ssr path.
    use tracing::Instrument;

    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    let client = CLIENT.get_or_init(|| {
        reqwest::ClientBuilder::new()
            .tcp_keepalive(std::time::Duration::from_secs(60))
            .build()
            .unwrap()
    });
    let req_parts = use_context::<Parts>().ok_or(AppError::ParamMissing)?;
    let headers = req_parts.headers;
    let hostname =
        std::env::var("HOSTNAME").unwrap_or_else(|_| "http://localhost:8080".to_string());
    let path = format!("{hostname}{path}");
    // headers.remove("Accept-Encoding");
    // this is only necessary because reqwest isn't updated to http 1.0- and I'm being lazy
    let mut new_map = reqwest::header::HeaderMap::new();
    for (name, value) in headers.into_iter().filter_map(|(name, value)| {
        Some((
            reqwest::header::HeaderName::from_lowercase(name?.as_str().as_bytes()).ok()?,
            reqwest::header::HeaderValue::from_bytes(value.as_bytes()).ok()?,
        ))
    }) {
        new_map.insert(name, value);
    }
    let request = client.delete(&path).headers(new_map).build()?;
    let response = client
        .execute(request)
        .await
        .instrument(tracing::trace_span!("HTTP FETCH"))
        .into_inner()
        .map_err(|e| {
            error!("Response {e}. {path}");
            e
        })?;
    let status = response.status();
    let json = response.text().await?;
    parse_internal_api_response(status, &json)
}

#[cfg(not(feature = "ssr"))]
#[instrument(skip())]
pub(crate) async fn fetch_api<T>(path: &str) -> AppResult<T>
where
    T: DeserializeOwned,
{
    use leptos::task::spawn_local;
    let (tx, rx) = flume::unbounded();

    spawn_local({
        let path = path.to_string();
        async move {
            let inner_impl = async move || -> AppResult<String> {
                let json: String = gloo_net::http::Request::get(&path)
                    // .abort_signal(abort_signal.as_ref())
                    .send()
                    .await
                    .inspect_err(|e| error!(error = %e, path, "Error making http request"))?
                    .text()
                    .await?;
                Ok(json)
            };
            let result = inner_impl().await;
            let _ = tx.send(result);
        }
    });
    let json = rx
        .into_recv_async()
        .await
        .expect("The channel to just work")?;
    deserialize(&json).inspect_err(|e| {
        error!(error = ?e, path, "Error deserializing");
    })
}

#[cfg(feature = "ssr")]
#[instrument(skip())]
pub(crate) async fn fetch_api<T>(path: &str) -> AppResult<T>
where
    T: serde::de::DeserializeOwned,
{
    // use the original headers of the scope
    // add the hostname when using the ssr path.
    use axum::http::request::Parts;
    use leptos::prelude::use_context;
    use tracing::Instrument;

    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    let client = CLIENT.get_or_init(|| {
        reqwest::ClientBuilder::new()
            .tcp_keepalive(std::time::Duration::from_secs(60))
            .build()
            .unwrap()
    });
    let req_parts = use_context::<Parts>().ok_or(AppError::ParamMissing)?;
    let headers = req_parts.headers;
    let hostname =
        std::env::var("HOSTNAME").unwrap_or_else(|_| "http://localhost:8080".to_string());
    let path = format!("{hostname}{path}");
    // this is only necessary because reqwest isn't updated to http 1.0- and I'm being lazy
    let mut new_map = reqwest::header::HeaderMap::new();
    for (name, value) in headers.into_iter().filter_map(|(name, value)| {
        Some((
            reqwest::header::HeaderName::from_lowercase(name?.as_str().as_bytes()).ok()?,
            reqwest::header::HeaderValue::from_bytes(value.as_bytes()).ok()?,
        ))
    }) {
        new_map.insert(name, value);
    }
    let request = client.get(&path).headers(new_map).build()?;
    let response = client
        .execute(request)
        .await
        .instrument(tracing::trace_span!("HTTP FETCH"))
        .into_inner()
        .inspect_err(|e| {
            error!(error = ?e, path, "Error doing leptos fetch");
        })?;
    let status = response.status();
    let json = response.text().await?;
    parse_internal_api_response(status, &json).inspect_err(|e| {
        // Only a *successful* response that fails to parse is a real bug worth
        // error-level reporting (GlitchTip). A non-success status is an
        // expected error response — notably the analyzer's transient 503
        // warm-up right after a deploy (issue 2218) — so log those quietly to
        // match the server side (`ultros/src/web/error.rs`).
        if status.is_success() {
            error!(error = ?e, path, json, "Error deserializing text");
        } else if status == reqwest::StatusCode::SERVICE_UNAVAILABLE {
            tracing::debug!(error = ?e, %status, path, "Internal API warming up");
        } else {
            tracing::warn!(error = ?e, %status, path, "Internal API error response");
        }
    })
}

#[cfg(not(feature = "ssr"))]
#[instrument(skip(json))]
pub(crate) async fn post_api<Y, T>(path: &str, json: Y) -> AppResult<T>
where
    Y: serde::Serialize + 'static,
    T: serde::de::DeserializeOwned,
{
    use leptos::task::spawn_local;

    let path = path.to_string();
    log::info!("making post request: {path}");
    let (tx, rx) = flume::unbounded::<AppResult<String>>();
    spawn_local(async move {
        let inner_impl = async move || -> AppResult<String> {
            tracing::info!("{}", &path);
            let body = serde_json::to_string(&json)
                .map_err(|e| anyhow::anyhow!("failed to serialize json body: {:?}", e))?;
            let json: String = gloo_net::http::Request::post(&path)
                .header("Content-Type", "application/json")
                .credentials(web_sys::RequestCredentials::Include)
                .body(body)
                .map_err(|e| anyhow::anyhow!("failed to set json body: {:?}", e))?
                .send()
                .await
                .inspect_err(|e| {
                    log::error!("{e}");
                })?
                .text()
                .await
                .inspect_err(|e| log::error!("{e}"))?;
            Ok(json)
        };
        let result = inner_impl().await;
        log::info!("sent result! {result:?}");
        tx.send(result).unwrap();
    });
    log::info!("spawn local rx");
    let json = rx
        .into_recv_async()
        .await
        .expect("The channel to just work")?;
    deserialize(&json)
}

#[cfg(feature = "ssr")]
#[instrument(skip(_json))]
pub(crate) async fn post_api<Y, T>(_path: &str, _json: Y) -> AppResult<T>
where
    Y: Serialize,
    T: Serialize,
{
    // This really only will be called by clients- I think.
    unreachable!("post_api should only be called on clients? I think...")
}

#[cfg(not(feature = "ssr"))]
#[instrument(skip(json))]
pub(crate) async fn patch_api<Y, T>(path: &str, json: Y) -> AppResult<T>
where
    Y: serde::Serialize + 'static,
    T: serde::de::DeserializeOwned,
{
    use leptos::task::spawn_local;

    let path = path.to_string();
    let (tx, rx) = flume::unbounded::<AppResult<String>>();
    spawn_local(async move {
        let inner_impl = async move || -> AppResult<String> {
            let body = serde_json::to_string(&json)
                .map_err(|e| anyhow::anyhow!("failed to serialize json body: {:?}", e))?;
            let json: String = gloo_net::http::Request::patch(&path)
                .header("Content-Type", "application/json")
                .credentials(web_sys::RequestCredentials::Include)
                .body(body)
                .map_err(|e| anyhow::anyhow!("failed to set json body: {:?}", e))?
                .send()
                .await
                .inspect_err(|e| {
                    log::error!("{e}");
                })?
                .text()
                .await
                .inspect_err(|e| log::error!("{e}"))?;
            Ok(json)
        };
        let result = inner_impl().await;
        tx.send(result).unwrap();
    });
    let json = rx
        .into_recv_async()
        .await
        .expect("The channel to just work")?;
    deserialize(&json)
}

#[cfg(feature = "ssr")]
#[instrument(skip(_json))]
pub(crate) async fn patch_api<Y, T>(_path: &str, _json: Y) -> AppResult<T>
where
    Y: Serialize,
    T: Serialize,
{
    // This really only will be called by clients- I think.
    unreachable!("patch_api should only be called on clients? I think...")
}

#[cfg(all(test, feature = "ssr"))]
mod ssr_response_tests {
    use super::parse_internal_api_response;
    use crate::error::AppError;
    use reqwest::StatusCode;
    use ultros_api_types::result::{ApiError, JsonErrorWrapper};

    /// Regression for GlitchTip issue 2218. The analyzer answers
    /// `503 + "Still warming up with data, unable to serve requests."` (plain
    /// text) during its post-deploy warm-up. The SSR fetch helper used to feed
    /// that body straight into `serde_json`, producing a misleading
    /// `AppError::Json("expected value at line 1 column 1")` logged at error
    /// level. A non-success status must yield a real API error and must never
    /// be classified as a JSON-deserialize failure.
    #[test]
    fn warmup_503_plaintext_is_not_a_json_error() {
        let body = "Analyzer Error: Still warming up with data, unable to serve requests.";
        let err = parse_internal_api_response::<i32>(StatusCode::SERVICE_UNAVAILABLE, body)
            .expect_err("a 503 body must not parse as a value");
        assert!(
            !matches!(err, AppError::Json(_)),
            "503 warm-up body must not be treated as malformed JSON, got {err:?}",
        );
        match err {
            AppError::ApiError(ApiError::Message(msg)) => {
                assert!(
                    msg.contains("warming up"),
                    "message should carry the body: {msg}"
                );
            }
            other => panic!("expected ApiError::Message, got {other:?}"),
        }
    }

    /// A structured error body (the API's `JsonErrorWrapper`) on a non-success
    /// status must round-trip to the matching typed error, not a generic string.
    #[test]
    fn structured_error_body_is_preserved() {
        let body = serde_json::to_string(&JsonErrorWrapper::ApiError(ApiError::NotFound)).unwrap();
        let err = parse_internal_api_response::<i32>(StatusCode::NOT_FOUND, &body)
            .expect_err("a 404 must be an error");
        assert_eq!(err, AppError::ApiError(ApiError::NotFound));
    }

    /// The happy path still deserializes the body into `T` on a 2xx.
    #[test]
    fn success_body_deserializes_value() {
        let value = parse_internal_api_response::<i32>(StatusCode::OK, "42").unwrap();
        assert_eq!(value, 42);
    }

    /// A 2xx whose body fails to deserialize is the one case that *is* a real
    /// bug — it must still surface as an error (so the caller error-logs it).
    #[test]
    fn success_body_with_garbage_is_an_error() {
        let err = parse_internal_api_response::<i32>(StatusCode::OK, "not json")
            .expect_err("garbage on a 200 is an error");
        assert!(matches!(err, AppError::Json(_)), "got {err:?}");
    }
}
