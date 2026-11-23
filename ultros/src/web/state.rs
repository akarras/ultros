//! Top-level Axum `WebState` plus the `FromRef` impls that let handlers extract
//! individual services with `State<T>` instead of the full `State<WebState>`.

use std::sync::Arc;

use axum::extract::FromRef;
use axum_extra::extract::cookie::Key;
use leptos::config::LeptosOptions;
use tokio_util::sync::CancellationToken;
use ultros_api_types::world_helper::WorldHelper;
use ultros_clickhouse::ClickHouseClient;
use ultros_db::{UltrosDb, world_data::world_cache::WorldCache};

use crate::character_claim::CharacterClaimService;
use crate::event::{EventReceivers, EventSenders};
use crate::market_client::MarketClient;
use crate::roles::Roles;
use crate::search_service::SearchService;
use crate::web::oauth::{AuthUserCache, DiscordAuthConfig};

#[derive(Clone)]
pub(crate) struct WebState {
    pub(crate) db: UltrosDb,
    pub(crate) key: Key,
    pub(crate) oauth_config: DiscordAuthConfig,
    pub(crate) user_cache: AuthUserCache,
    pub(crate) event_receivers: EventReceivers,
    pub(crate) event_senders: EventSenders,
    pub(crate) world_cache: Arc<WorldCache>,
    /// Common variant of world_cache. Maybe get rid of world_cache?
    pub(crate) world_helper: Arc<WorldHelper>,
    /// Typed market boundary: in-process when `ULTROS_ROLES` includes `market`,
    /// or coarse HTTP/2 against `ULTROS_MARKET_URL` on stateless `web` replicas.
    pub(crate) market_client: Arc<dyn MarketClient>,
    pub(crate) roles: Roles,
    pub(crate) character_claim: CharacterClaimService,
    pub(crate) leptos_options: LeptosOptions,
    pub(crate) search_service: SearchService,
    pub(crate) token: CancellationToken,
    /// ClickHouse client for per-item analytical queries directly served by web.
    pub(crate) ch_client: ClickHouseClient,
    /// Absorbs bursts of identical chart requests (`floor_history`,
    /// `undercut_pressure`, `price_density`). See [`crate::web::price_series_cache`].
    pub(crate) price_series_cache: crate::web::price_series_cache::PriceSeriesCache,
    /// The local-first list document's server side: merge path, activity
    /// classification, and the legacy-event/relay fanout.
    pub(crate) list_sync: crate::lists::ListSync,
}

impl FromRef<WebState> for UltrosDb {
    fn from_ref(input: &WebState) -> Self {
        input.db.clone()
    }
}

impl FromRef<WebState> for Key {
    fn from_ref(input: &WebState) -> Self {
        input.key.clone()
    }
}

impl FromRef<WebState> for DiscordAuthConfig {
    fn from_ref(input: &WebState) -> Self {
        input.oauth_config.clone()
    }
}

impl FromRef<WebState> for AuthUserCache {
    fn from_ref(input: &WebState) -> Self {
        input.user_cache.clone()
    }
}

impl FromRef<WebState> for EventReceivers {
    fn from_ref(input: &WebState) -> Self {
        input.event_receivers.clone()
    }
}

impl FromRef<WebState> for Arc<WorldCache> {
    fn from_ref(input: &WebState) -> Self {
        input.world_cache.clone()
    }
}

impl FromRef<WebState> for Arc<WorldHelper> {
    fn from_ref(input: &WebState) -> Self {
        input.world_helper.clone()
    }
}

impl FromRef<WebState> for Arc<dyn MarketClient> {
    fn from_ref(input: &WebState) -> Self {
        input.market_client.clone()
    }
}

impl FromRef<WebState> for Roles {
    fn from_ref(input: &WebState) -> Self {
        input.roles
    }
}

impl FromRef<WebState> for EventSenders {
    fn from_ref(input: &WebState) -> Self {
        input.event_senders.clone()
    }
}

impl FromRef<WebState> for CharacterClaimService {
    fn from_ref(input: &WebState) -> Self {
        input.character_claim.clone()
    }
}

impl FromRef<WebState> for LeptosOptions {
    fn from_ref(input: &WebState) -> Self {
        input.leptos_options.clone()
    }
}

impl FromRef<WebState> for SearchService {
    fn from_ref(input: &WebState) -> Self {
        input.search_service.clone()
    }
}

impl FromRef<WebState> for ClickHouseClient {
    fn from_ref(input: &WebState) -> Self {
        input.ch_client.clone()
    }
}

impl FromRef<WebState> for crate::web::price_series_cache::PriceSeriesCache {
    fn from_ref(input: &WebState) -> Self {
        input.price_series_cache.clone()
    }
}

/// Long-lived handlers (the websockets) need the same token the serve loop
/// watches, so they can close themselves instead of holding graceful shutdown
/// open. See [`crate::web::shutdown`].
impl FromRef<WebState> for CancellationToken {
    fn from_ref(input: &WebState) -> Self {
        input.token.clone()
    }
}

impl FromRef<WebState> for crate::lists::ListSync {
    fn from_ref(input: &WebState) -> Self {
        input.list_sync.clone()
    }
}
