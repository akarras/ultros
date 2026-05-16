//! Top-level Axum `WebState` plus the `FromRef` impls that let handlers extract
//! individual services with `State<T>` instead of the full `State<WebState>`.

use std::sync::Arc;

use axum::extract::FromRef;
use axum_extra::extract::cookie::Key;
use leptos::config::LeptosOptions;
use tokio_util::sync::CancellationToken;
use ultros_api_types::world_helper::WorldHelper;
use ultros_db::{UltrosDb, world_data::world_cache::WorldCache};

use ultros_clickhouse::ClickHouseClient;

use crate::analyzer_service::AnalyzerService;
use crate::event::{EventReceivers, EventSenders};
use crate::search_service::SearchService;
use crate::web::character_verifier_service::CharacterVerifierService;
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
    pub(crate) analyzer_service: AnalyzerService,
    pub(crate) character_verification: CharacterVerifierService,
    pub(crate) leptos_options: LeptosOptions,
    pub(crate) search_service: SearchService,
    pub(crate) token: CancellationToken,
    /// ClickHouse client for analytical queries (Phase 1+ uses this; Phase 0
    /// only writes via the analyzer's dual-write path).
    pub(crate) ch_client: ClickHouseClient,
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

impl FromRef<WebState> for AnalyzerService {
    fn from_ref(input: &WebState) -> Self {
        input.analyzer_service.clone()
    }
}

impl FromRef<WebState> for EventSenders {
    fn from_ref(input: &WebState) -> Self {
        input.event_senders.clone()
    }
}

impl FromRef<WebState> for CharacterVerifierService {
    fn from_ref(input: &WebState) -> Self {
        input.character_verification.clone()
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
