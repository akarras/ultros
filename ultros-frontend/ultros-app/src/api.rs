use futures::future::try_join_all;
use itertools::Itertools;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::collections::HashMap;
use tracing::error;
use tracing::instrument;
use ultros_api_types::{
    ActiveListing, CurrentlyShownItem, FfxivCharacter,
    alert::{
        Alert, AlertEvent, CreateAlertRequest, CreateEndpointRequest,
        CreatePushSubscriptionRequest, DeleteEndpointResponse, DiscordWritableGuild, Endpoint,
        ResendResult, UpdateAlertRequest, UpdateEndpointRequest, VapidPublicKey,
    },
    character_purchases::CharacterPurchaseHistory,
    cheapest_listings::{CheapestListings, CheapestListingsMap},
    item_stats::ItemStatsResponse,
    list::{
        CreateInvite, CreateList, List, ListActivity, ListInvite, ListItem, ListSharedGroup,
        ListSharedUser, ListWithPermission, ShareListGroup, ShareListUser,
    },
    market_heat::MarketHeatResponse,
    market_pulse::MarketPulseDto,
    price_density::PriceDensity,
    price_series::{HqFilter, PriceSeries, SeriesGroup},
    recent_sales::RecentSales,
    resale_quality::{ResaleQualityRequest, ResaleQualityResponse},
    result::JsonErrorWrapper,
    retainer::{Retainer, RetainerListings},
    sale_stats::BulkSaleStats,
    search::SearchResult,
    sparklines::{MoversResponse, SparklinesRequest, SparklinesResponse},
    trends::TrendsData,
    user::{
        AssignRetainerCharacter, OwnedRetainer, UserData, UserRetainerListings, UserRetainers,
        group::{
            CreateGroup, CreateGroupFromGuild, CreateGroupInvite, DiscordManageableGuild,
            GroupInvite, UserGroup, UserGroupMember,
        },
    },
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

/// Pre-bucketed price series for the item chart. The payload size tracks the
/// requested window rather than the item's sale count, so this is safe at
/// full history. (The raw-sales client wrapper for `/api/v1/extended_history`
/// was removed when the chart moved to this endpoint — the HTTP route is
/// still registered server-side, just no longer called from here.)
pub(crate) async fn get_price_series(
    item_id: i32,
    world: &str,
    group: SeriesGroup,
    hq: HqFilter,
    range: Option<(i64, i64)>,
) -> AppResult<PriceSeries> {
    if item_id == 0 {
        return Err(AppError::NoItem);
    }
    let mut url = format!(
        "/api/v1/price_series/{world}/{item_id}?group={}&hq={}",
        group.as_str(),
        hq.as_str()
    );
    if let Some((from, to)) = range {
        url.push_str(&format!("&from={from}&to={to}"));
    }
    fetch_api(&url).await
}

/// Time × price sale-count grid for the chart's density mode. Fetched only
/// while density mode is active — see the gated LocalResource in item_view.
pub(crate) async fn get_price_density(
    item_id: i32,
    world: &str,
    hq: HqFilter,
    range: Option<(i64, i64)>,
    price_bins: u16,
) -> AppResult<PriceDensity> {
    if item_id == 0 {
        return Err(AppError::NoItem);
    }
    let mut url = format!(
        "/api/v1/price_density/{world}/{item_id}?hq={}&price_bins={price_bins}",
        hq.as_str()
    );
    if let Some((from, to)) = range {
        url.push_str(&format!("&from={from}&to={to}"));
    }
    fetch_api(&url).await
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

pub(crate) async fn get_cheapest_listings_live(
    world_name: &str,
    refresh_version: u64,
) -> AppResult<CheapestListings> {
    if refresh_version == 0 {
        get_cheapest_listings(world_name).await
    } else {
        fetch_api(&format!(
            "/api/v1/cheapest/{world_name}?rt={refresh_version}"
        ))
        .await
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct ResaleStatsDto {
    pub(crate) profit: i32,
    pub(crate) item_id: i32,
    #[serde(default)]
    pub(crate) hq: bool,
    pub(crate) sold_within: String,
    pub(crate) return_on_investment: f32,
    /// Gil paid. `profit` is post-tax, so `buy_price + profit` is the take,
    /// not the list price — use `est_sale_price` for the latter.
    #[serde(default)]
    pub(crate) buy_price: i32,
    /// Pre-tax gil to list at.
    #[serde(default)]
    pub(crate) est_sale_price: i32,
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
    // Buffer-derived stats. Present on every row, unlike the deep-scan
    // fields above — which is why the card's credibility signals use these.
    #[serde(default)]
    pub(crate) velocity_per_day: Option<f32>,
    #[serde(default)]
    pub(crate) buffer_sale_count: u8,
    #[serde(default)]
    pub(crate) recent_price_low: i32,
    #[serde(default)]
    pub(crate) recent_price_high: i32,
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
    /// Reject rows selling slower than this many per day.
    pub min_velocity: Option<f32>,
    /// Reject rows with fewer than this many sales in the recent buffer.
    pub min_buffer_sales: Option<u8>,
    /// Reject rows above this ROI percentage.
    pub max_roi: Option<f32>,
}

pub(crate) async fn get_best_deals(
    world_name: &str,
    params: BestDealsParams,
) -> AppResult<Vec<ResaleStatsDto>> {
    let mut qs: Vec<String> = Vec::with_capacity(7);
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
    if let Some(v) = params.min_velocity {
        qs.push(format!("min_velocity={v}"));
    }
    if let Some(n) = params.min_buffer_sales {
        qs.push(format!("min_buffer_sales={n}"));
    }
    if let Some(r) = params.max_roi {
        qs.push(format!("max_roi={r}"));
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

/// Bulk sale-history statistics (min/median/avg per item) for a world,
/// datacenter, or region — the recipe analyzer's selectable cost basis.
pub(crate) async fn get_sale_stats(scope_name: &str, window_days: u16) -> AppResult<BulkSaleStats> {
    fetch_api(&format!(
        "/api/v1/sale_stats/{scope_name}?window={window_days}"
    ))
    .await
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

/// Claims the given character for the logged-in user.
///
/// Claims aren't verified — they only group the user's retainers — so this
/// takes effect immediately and returns the claimed character.
pub(crate) async fn claim_character(id: i32) -> AppResult<FfxivCharacter> {
    fetch_api(&format!("/api/v1/characters/claim/{id}")).await
}

pub(crate) async fn unclaim_character(id: i32) -> AppResult<(i32, String)> {
    fetch_api(&format!("/api/v1/characters/unclaim/{id}")).await
}

/// Purchase history for one of the logged-in user's claimed characters.
///
/// Owned-characters only, enforced server-side: Ultros knows a buyer only by
/// the bare character name Universalis reports, so the claim is what supplies
/// the world that scopes the search — and aggregating a name's purchases is a
/// spending profile, which is not something to hand out for arbitrary names.
pub(crate) async fn get_character_purchases(
    character_id: i32,
) -> AppResult<CharacterPurchaseHistory> {
    fetch_api(&format!("/api/v1/characters/{character_id}/purchases")).await
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

#[derive(Serialize)]
pub(crate) struct BulkHqUpdate {
    pub(crate) ids: Vec<i32>,
    pub(crate) hq: Option<bool>,
}

pub(crate) async fn edit_list_items_hq(ids: Vec<i32>, hq: Option<bool>) -> AppResult<()> {
    post_api("/api/v1/list/item/hq", BulkHqUpdate { ids, hq }).await
}

pub(crate) async fn get_groups() -> AppResult<Vec<UserGroup>> {
    fetch_api("/api/v1/group").await
}

pub(crate) async fn create_group(group: CreateGroup) -> AppResult<()> {
    post_api("/api/v1/group/create", group).await
}

pub(crate) async fn delete_group(id: i32) -> AppResult<()> {
    delete_api(&format!("/api/v1/group/{id}")).await
}

/// Discord servers the logged-in user could turn into a group. Hits Discord on
/// the server side, so only call this when the guild picker is actually open.
pub(crate) async fn list_manageable_discord_guilds() -> AppResult<Vec<DiscordManageableGuild>> {
    fetch_api("/api/v1/group/discord-guilds").await
}

pub(crate) async fn create_group_from_guild(guild_id: i64) -> AppResult<UserGroup> {
    post_api(
        "/api/v1/group/create-from-guild",
        CreateGroupFromGuild { guild_id },
    )
    .await
}

pub(crate) async fn get_group_members(id: i32) -> AppResult<Vec<UserGroupMember>> {
    fetch_api(&format!("/api/v1/group/{id}/members")).await
}

pub(crate) async fn add_group_member(group_id: i32, user_id: u64) -> AppResult<()> {
    post_api(
        &format!("/api/v1/group/{group_id}/member/add/{user_id}"),
        (),
    )
    .await
}

pub(crate) async fn remove_group_member(group_id: i32, user_id: u64) -> AppResult<()> {
    delete_api(&format!("/api/v1/group/{group_id}/member/remove/{user_id}")).await
}

pub(crate) async fn get_group_invites(group_id: i32) -> AppResult<Vec<GroupInvite>> {
    fetch_api(&format!("/api/v1/group/{group_id}/invites")).await
}

pub(crate) async fn create_group_invite(
    group_id: i32,
    invite: CreateGroupInvite,
) -> AppResult<GroupInvite> {
    post_api(&format!("/api/v1/group/{group_id}/invite/create"), invite).await
}

/// Returns the id of the group joined, so the caller can navigate to it.
pub(crate) async fn use_group_invite(invite_id: String) -> AppResult<i32> {
    post_api(&format!("/api/v1/group-invite/{invite_id}/use"), ()).await
}

pub(crate) async fn delete_group_invite(invite_id: String) -> AppResult<()> {
    delete_api(&format!("/api/v1/group-invite/{invite_id}")).await
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

pub(crate) async fn assign_retainer_character(
    owned_retainer_id: i32,
    character_id: Option<i32>,
) -> AppResult<()> {
    post_api(
        &format!("/api/v1/retainer/{owned_retainer_id}/character"),
        AssignRetainerCharacter { character_id },
    )
    .await
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

pub(crate) async fn delete_endpoint(id: i32) -> AppResult<DeleteEndpointResponse> {
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

/// Headers that must not be copied from the inbound browser request onto the
/// outbound internal API request.
///
/// The SSR path re-issues each API call against [`internal_api_origin`], which
/// is no longer the public origin — but it still must not carry the inbound
/// `host`, and the reason it originally mattered is worth keeping: when the
/// outbound call did travel back out through the CDN,
/// copying the inbound `host` header onto a request aimed at a different URL
/// makes the CDN reject it: a `Host` that does not match the edge certificate
/// answers `403 Forbidden`, and on a pooled TLS connection (the client below
/// sets a 60s keepalive, so connections are reused) a `Host` that disagrees
/// with the connection's SNI answers `421 Misdirected Request`. Both statuses
/// were live in production against `/api/v1/cheapest/North-America` — the
/// failure is silent to the visitor, because a failed resource still renders,
/// just with no data in it, so it only ever showed up as GlitchTip issue 2209
/// ("Error doing leptos fetch") and as breadcrumbs on unrelated events.
///
/// The rest fall into three groups:
/// - hop-by-hop headers, which are per-connection and must never be forwarded
///   (RFC 9110 §7.6.1);
/// - body-framing headers, which would describe a body we are not resending;
/// - CDN/proxy trust headers, which the edge re-derives for the new request and
///   must not receive secondhand from a client.
///
/// This is deliberately a denylist: the inbound `cookie` carries the visitor's
/// session and `accept-language` carries their locale, and an allowlist would
/// silently break auth or i18n the first time a new header started mattering.
#[cfg(feature = "ssr")]
const NON_FORWARDABLE_HEADERS: &[&str] = &[
    // Routing: the whole reason this function exists.
    "host",
    // Hop-by-hop (RFC 9110 §7.6.1).
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
    // Body framing — these requests carry no body.
    "content-length",
    "content-type",
    // reqwest negotiates (and decodes) its own encodings; forwarding the
    // browser's list can hand back a body reqwest will not decode.
    "accept-encoding",
    // Re-derived by the edge for the outbound request.
    "cf-connecting-ip",
    "cf-ipcountry",
    "cf-ray",
    "cf-visitor",
    "x-forwarded-for",
    "x-forwarded-host",
    "x-forwarded-proto",
    "x-real-ip",
];

/// Turn the address the server is bound to into an origin it can call itself
/// on, or `None` if it does not look like an `addr:port` pair.
///
/// `0.0.0.0` and `[::]` are the *unspecified* address — "listen on every
/// interface". They are a valid bind target and not a valid destination, so
/// they map to the matching loopback address. Anything else is already a
/// concrete address the process answers on and is used verbatim.
#[cfg(feature = "ssr")]
fn loopback_origin_from_site_addr(site_addr: &str) -> Option<String> {
    let (host, port) = site_addr.trim().rsplit_once(':')?;
    if port.is_empty() || !port.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    // IPv6 literals arrive bracketed (`[::]:8080`) and stay bracketed in a URL.
    let host = match host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
        Some("::" | "::0" | "0:0:0:0:0:0:0:0") => "[::1]",
        Some(_) => host,
        None if host.is_empty() || host == "0.0.0.0" => "127.0.0.1",
        None => host,
    };
    Some(format!("http://{host}:{port}"))
}

/// Pick the origin the SSR renderer issues its own API calls against.
///
/// Precedence: an explicit override, then the address the server is bound to,
/// then the public `HOSTNAME`, then a development default.
///
/// This used to be `HOSTNAME` alone, and `HOSTNAME` is the app's *public* URL
/// (`https://ultros.app` in production) — it has to stay that way, OAuth
/// redirects are built from it. Using it here too meant every server-rendered
/// page re-fetched its own API by leaving the box: DNS to Cloudflare, a fresh
/// TLS handshake, back in through the edge, into the very same process.
///
/// Measured on the production host: `/api/v1/cheapest/Europe` answers in 8-20ms
/// over loopback versus ~60ms through the edge when the edge is healthy — and
/// the edge is not always healthy. A single 4h26m container log window
/// (2026-08-10 08:53→13:19) held **429** `source: TimedOut` failures against
/// this module's 10s client budget, concentrated in two minutes (10:53-10:54)
/// where the edge stalled: 88 for `cheapest/Europe`, 53 for
/// `cheapest/North-America`, 52 for `retainer/listings/{id}`, and a long tail of
/// `listings/{world}/{id}` — CN and JP worlds especially. Those are GlitchTip
/// issues 2209 ("Error doing leptos fetch") and 2210 ("Error getting value").
///
/// A failed SSR fetch does not error the page; the resource still renders, just
/// empty. So the visible symptom is a page that silently loses a section, which
/// is why this survived so long. Going over loopback removes the entire class:
/// no DNS, no TLS handshake, no CDN, no WAF, no edge rate limit between the
/// process and its own API.
///
/// It is *not* a fix for the SSR panic flood (GlitchTip 6876/6886/6888/6895) —
/// those run at a steady ~120/min across the whole window and do not correlate
/// with the timeout bursts.
///
/// `HOSTNAME` is kept as a fallback so an unusual deployment still works, and
/// its trailing slash is trimmed — `fly.toml` sets `https://ultros.app/`, which
/// concatenated with a leading-slash path produced a double-slash URL.
#[cfg(feature = "ssr")]
fn resolve_internal_api_origin(
    explicit: Option<&str>,
    site_addr: Option<&str>,
    hostname: Option<&str>,
) -> String {
    fn set(value: Option<&str>) -> Option<&str> {
        value.map(str::trim).filter(|value| !value.is_empty())
    }

    if let Some(explicit) = set(explicit) {
        return explicit.trim_end_matches('/').to_owned();
    }
    if let Some(origin) = set(site_addr).and_then(loopback_origin_from_site_addr) {
        return origin;
    }
    if let Some(hostname) = set(hostname) {
        return hostname.trim_end_matches('/').to_owned();
    }
    "http://localhost:8080".to_owned()
}

/// The origin every SSR-side API call is issued against, resolved once.
///
/// See [`resolve_internal_api_origin`] for why this is not simply `HOSTNAME`.
#[cfg(feature = "ssr")]
fn internal_api_origin() -> &'static str {
    static ORIGIN: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    ORIGIN.get_or_init(|| {
        let explicit = std::env::var("ULTROS_INTERNAL_API_ORIGIN").ok();
        let site_addr = std::env::var("LEPTOS_SITE_ADDR").ok();
        let hostname = std::env::var("HOSTNAME").ok();
        let origin = resolve_internal_api_origin(
            explicit.as_deref(),
            site_addr.as_deref(),
            hostname.as_deref(),
        );
        tracing::info!(%origin, "resolved SSR internal API origin");
        origin
    })
}

/// Copy the inbound request's headers into a header map suitable for the
/// outbound internal API call, dropping everything in [`NON_FORWARDABLE_HEADERS`].
///
/// `HeaderMap`'s iterator yields `None` for the name of a repeated header's
/// second and subsequent values, so the name is carried forward and the value
/// appended — otherwise every multi-valued header would be truncated to its
/// first value.
#[cfg(feature = "ssr")]
fn forwardable_headers(headers: axum::http::HeaderMap) -> reqwest::header::HeaderMap {
    let mut new_map = reqwest::header::HeaderMap::new();
    let mut current: Option<reqwest::header::HeaderName> = None;
    for (name, value) in headers.into_iter() {
        if let Some(name) = name {
            current = reqwest::header::HeaderName::from_lowercase(name.as_str().as_bytes())
                .ok()
                .filter(|name| !NON_FORWARDABLE_HEADERS.contains(&name.as_str()));
        }
        let Some(name) = current.clone() else {
            continue;
        };
        if let Ok(value) = reqwest::header::HeaderValue::from_bytes(value.as_bytes()) {
            new_map.append(name, value);
        }
    }
    new_map
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
            .timeout(std::time::Duration::from_secs(10))
            .tcp_keepalive(std::time::Duration::from_secs(60))
            .build()
            .unwrap()
    });
    let req_parts = use_context::<Parts>().ok_or(AppError::ParamMissing)?;
    let headers = req_parts.headers;
    let path = format!("{}{path}", internal_api_origin());
    let request = client
        .delete(&path)
        .headers(forwardable_headers(headers))
        .build()?;
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
            .timeout(std::time::Duration::from_secs(10))
            .tcp_keepalive(std::time::Duration::from_secs(60))
            .build()
            .unwrap()
    });
    let req_parts = use_context::<Parts>().ok_or(AppError::ParamMissing)?;
    let headers = req_parts.headers;
    let path = format!("{}{path}", internal_api_origin());
    let request = client
        .get(&path)
        .headers(forwardable_headers(headers))
        .build()?;
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

    /// An unauthenticated response must surface as `NotAuthenticated` so
    /// callers can act on it — e.g. the list-invite login redirect in
    /// `routes/lists.rs` matches this exact variant.
    #[test]
    fn unauthenticated_401_maps_to_not_authenticated() {
        let body =
            serde_json::to_string(&JsonErrorWrapper::ApiError(ApiError::NotAuthenticated)).unwrap();
        let err = parse_internal_api_response::<i32>(StatusCode::UNAUTHORIZED, &body)
            .expect_err("a 401 must be an error");
        assert_eq!(err, AppError::ApiError(ApiError::NotAuthenticated));
    }

    /// Safety proof for moving `ApiError::NoAuthCookie` from `200` to `401`
    /// server-side (`ultros/src/web/error.rs`): callers see the *same*
    /// `AppError` either way, because the 200 path recovers the wrapper through
    /// `deserialize`'s fallback and the 401 path maps the status explicitly.
    ///
    /// The difference is only in how it gets *reported*: on a 200 the SSR fetch
    /// helper takes its `status.is_success()` branch and logs
    /// "Error deserializing text" at error level (GlitchTip noise), while a 401
    /// is a plain expected error response.
    #[test]
    fn unauthenticated_200_and_401_produce_the_same_app_error() {
        let body =
            serde_json::to_string(&JsonErrorWrapper::ApiError(ApiError::NotAuthenticated)).unwrap();
        let legacy_200 = parse_internal_api_response::<i32>(StatusCode::OK, &body)
            .expect_err("an auth failure is always an error");
        let fixed_401 = parse_internal_api_response::<i32>(StatusCode::UNAUTHORIZED, &body)
            .expect_err("an auth failure is always an error");
        assert_eq!(
            legacy_200, fixed_401,
            "changing the status must not change what callers observe"
        );
        assert_eq!(fixed_401, AppError::ApiError(ApiError::NotAuthenticated));
    }

    /// Regression for GlitchTip issue 2210 ("Error getting value"), 6584 events.
    ///
    /// A world segment the API cannot resolve — in production, mojibake where
    /// the world name belongs, e.g.
    /// `/api/v1/listings/綛糸襲臂ゅ甥/42525` — comes back as a 404
    /// carrying `WorldCacheError`'s message. This helper already logs it at
    /// warn, so whatever awaits the resource must be able to tell that the API
    /// *answered* and skip a second, error-level report.
    #[test]
    fn unresolvable_world_404_is_an_api_response() {
        let body = serde_json::to_string(&JsonErrorWrapper::ApiError(ApiError::Message(
            "Name lookup error 綛糸襲臂ゅ甥".to_string(),
        )))
        .unwrap();
        let err = parse_internal_api_response::<i32>(StatusCode::NOT_FOUND, &body)
            .expect_err("a 404 must be an error");
        assert!(
            err.is_api_response(),
            "a 404 for a bad world name is the API answering, got {err:?}"
        );
    }

    /// The counterpart that must keep error-level reporting: a 2xx whose body
    /// will not deserialize is a real bug and is logged nowhere else.
    #[test]
    fn malformed_success_body_is_not_an_api_response() {
        let err = parse_internal_api_response::<i32>(StatusCode::OK, "not json")
            .expect_err("garbage on a 200 is an error");
        assert!(
            !err.is_api_response(),
            "a malformed 200 body is our own failure, got {err:?}"
        );
    }
}

#[cfg(all(test, feature = "ssr"))]
mod ssr_origin_tests {
    use super::{loopback_origin_from_site_addr, resolve_internal_api_origin};

    /// The production configuration, and the whole point of the change.
    ///
    /// The container runs with `HOSTNAME=https://ultros.app` and
    /// `LEPTOS_SITE_ADDR=0.0.0.0:8080`. Before this, every SSR fetch went to the
    /// public origin: out to Cloudflare and back into the same process, 10s
    /// budget, hundreds of `TimedOut` errors per log window. It must stay on the
    /// box.
    #[test]
    fn production_env_resolves_to_loopback_not_the_public_origin() {
        let origin =
            resolve_internal_api_origin(None, Some("0.0.0.0:8080"), Some("https://ultros.app"));
        assert_eq!(origin, "http://127.0.0.1:8080");
        assert!(
            !origin.contains("ultros.app"),
            "the SSR loopback must not leave the machine: {origin}"
        );
    }

    /// `LEPTOS_SITE_ADDR` is what the server actually binds, so it wins over the
    /// public `HOSTNAME` — but an operator can still pin the origin by hand.
    #[test]
    fn explicit_override_wins_over_everything() {
        assert_eq!(
            resolve_internal_api_origin(
                Some("http://api.internal:9000"),
                Some("0.0.0.0:8080"),
                Some("https://ultros.app"),
            ),
            "http://api.internal:9000"
        );
    }

    /// Unset in development (`cargo leptos serve` may not export it), in which
    /// case behaviour is exactly what it was before: `HOSTNAME`, then the
    /// localhost default. Blank strings count as unset — an env var set to the
    /// empty string would otherwise resolve to an origin-less URL.
    #[test]
    fn falls_back_through_hostname_then_the_dev_default() {
        assert_eq!(
            resolve_internal_api_origin(None, None, Some("http://localhost:3000")),
            "http://localhost:3000"
        );
        assert_eq!(
            resolve_internal_api_origin(Some(""), Some("  "), Some("")),
            "http://localhost:8080"
        );
        assert_eq!(
            resolve_internal_api_origin(None, None, None),
            "http://localhost:8080"
        );
    }

    /// `fly.toml` sets `HOSTNAME = "https://ultros.app/"`. Concatenated with a
    /// leading-slash path that produced `https://ultros.app//api/v1/...`.
    #[test]
    fn a_trailing_slash_on_the_origin_is_trimmed() {
        assert_eq!(
            resolve_internal_api_origin(None, None, Some("https://ultros.app/")),
            "https://ultros.app"
        );
        assert_eq!(
            format!("{}{}", "https://ultros.app", "/api/v1/cheapest/Europe"),
            "https://ultros.app/api/v1/cheapest/Europe"
        );
    }

    /// The unspecified address is a valid thing to *bind* and not a valid thing
    /// to *connect to*, so it has to become the matching loopback address. A
    /// concrete bind address is already reachable and is used as-is.
    #[test]
    fn the_unspecified_address_becomes_loopback() {
        for (addr, expected) in [
            ("0.0.0.0:8080", "http://127.0.0.1:8080"),
            ("[::]:8080", "http://[::1]:8080"),
            ("[::0]:3000", "http://[::1]:3000"),
            (":8080", "http://127.0.0.1:8080"),
            ("127.0.0.1:8080", "http://127.0.0.1:8080"),
            ("localhost:3000", "http://localhost:3000"),
            ("[::1]:3000", "http://[::1]:3000"),
            ("192.168.1.5:8080", "http://192.168.1.5:8080"),
            (" 0.0.0.0:8080 ", "http://127.0.0.1:8080"),
        ] {
            assert_eq!(
                loopback_origin_from_site_addr(addr).as_deref(),
                Some(expected),
                "{addr}"
            );
        }
    }

    /// Anything that is not an `addr:port` pair must fall through to the next
    /// source rather than produce a URL that cannot be connected to.
    #[test]
    fn a_malformed_site_addr_falls_through() {
        for addr in ["0.0.0.0", "", "http://0.0.0.0:8080/", "0.0.0.0:", "8080"] {
            assert_eq!(loopback_origin_from_site_addr(addr), None, "{addr}");
        }
        assert_eq!(
            resolve_internal_api_origin(None, Some("0.0.0.0"), Some("https://ultros.app")),
            "https://ultros.app"
        );
    }
}

#[cfg(all(test, feature = "ssr"))]
mod ssr_header_tests {
    use super::forwardable_headers;
    use axum::http::HeaderMap;
    use axum::http::header::{HeaderName, HeaderValue};

    fn inbound(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.append(
                HeaderName::from_lowercase(name.as_bytes()).unwrap(),
                HeaderValue::from_str(value).unwrap(),
            );
        }
        map
    }

    /// The bug this module exists for. The SSR path re-issues every API call
    /// against `HOSTNAME` (the public origin in production), so copying the
    /// inbound `host` verbatim aims a request at one URL while telling the CDN
    /// it is for another. Reproduced against production: a request to
    /// `https://ultros.app/api/v1/cheapest/North-America` carrying
    /// `Host: boxbox` answers `403 Forbidden` from the edge, byte-for-byte the
    /// body seen in the GlitchTip breadcrumbs; on a reused TLS connection the
    /// same mismatch answers `421 Misdirected Request`.
    #[test]
    fn host_is_never_forwarded() {
        let out = forwardable_headers(inbound(&[("host", "boxbox"), ("cookie", "session=abc123")]));
        assert!(
            !out.contains_key("host"),
            "the inbound host would misroute the outbound call: {out:?}"
        );
    }

    /// The session cookie is the entire reason the inbound headers are
    /// forwarded at all — dropping it would log every SSR render out.
    #[test]
    fn auth_and_locale_headers_survive() {
        let out = forwardable_headers(inbound(&[
            ("host", "boxbox"),
            ("cookie", "session=abc123"),
            ("accept-language", "de-DE,de;q=0.9"),
            ("user-agent", "Mozilla/5.0"),
        ]));
        assert_eq!(out.get("cookie").unwrap(), "session=abc123");
        assert_eq!(out.get("accept-language").unwrap(), "de-DE,de;q=0.9");
        assert_eq!(out.get("user-agent").unwrap(), "Mozilla/5.0");
    }

    /// Hop-by-hop headers are per-connection (RFC 9110 §7.6.1) and describe the
    /// browser's connection to the edge, not ours to the API. `accept-encoding`
    /// is dropped so reqwest negotiates an encoding it can actually decode —
    /// there was a commented-out `headers.remove("Accept-Encoding")` sitting in
    /// this file, which is the same problem noticed and never finished.
    #[test]
    fn hop_by_hop_and_framing_headers_are_dropped() {
        let out = forwardable_headers(inbound(&[
            ("connection", "keep-alive"),
            ("keep-alive", "timeout=5"),
            ("transfer-encoding", "chunked"),
            ("upgrade", "websocket"),
            ("te", "trailers"),
            ("content-length", "42"),
            ("accept-encoding", "gzip, br, zstd"),
            ("cookie", "session=abc123"),
        ]));
        for dropped in [
            "connection",
            "keep-alive",
            "transfer-encoding",
            "upgrade",
            "te",
            "content-length",
            "accept-encoding",
        ] {
            assert!(
                !out.contains_key(dropped),
                "{dropped} must not be forwarded"
            );
        }
        assert_eq!(out.get("cookie").unwrap(), "session=abc123");
    }

    /// A client must not be able to hand the edge its own provenance headers on
    /// a request the edge is meant to attribute to us.
    #[test]
    fn proxy_trust_headers_are_dropped() {
        let out = forwardable_headers(inbound(&[
            ("cf-connecting-ip", "1.2.3.4"),
            ("x-forwarded-for", "1.2.3.4"),
            ("x-forwarded-host", "evil.example"),
            ("x-real-ip", "1.2.3.4"),
            ("accept", "application/json"),
        ]));
        for dropped in [
            "cf-connecting-ip",
            "x-forwarded-for",
            "x-forwarded-host",
            "x-real-ip",
        ] {
            assert!(
                !out.contains_key(dropped),
                "{dropped} must not be forwarded"
            );
        }
        assert_eq!(out.get("accept").unwrap(), "application/json");
    }

    /// `HeaderMap`'s iterator reports `None` for the name of a repeated header's
    /// second and later values. The previous loop used `name?` inside a
    /// `filter_map`, so it silently discarded them and kept only the first.
    #[test]
    fn repeated_header_values_are_all_forwarded() {
        let out = forwardable_headers(inbound(&[
            ("accept-language", "de-DE"),
            ("accept-language", "en-US"),
        ]));
        let values: Vec<_> = out.get_all("accept-language").iter().collect();
        assert_eq!(values, vec!["de-DE", "en-US"]);
    }

    /// The continuation values of a *dropped* repeated header must be dropped
    /// too, rather than latching onto whichever name was forwarded last.
    #[test]
    fn continuation_values_of_a_dropped_header_are_also_dropped() {
        let out = forwardable_headers(inbound(&[
            ("cookie", "session=abc123"),
            ("x-forwarded-for", "1.2.3.4"),
            ("x-forwarded-for", "5.6.7.8"),
        ]));
        assert!(!out.contains_key("x-forwarded-for"));
        let cookies: Vec<_> = out.get_all("cookie").iter().collect();
        assert_eq!(
            cookies,
            vec!["session=abc123"],
            "leaked into cookie: {out:?}"
        );
    }
}
