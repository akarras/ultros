use axum::{
    extract::{FromRef, FromRequestParts, Query, State},
    http::request::Parts,
    response::Redirect,
};
use axum_extra::extract::{
    PrivateCookieJar,
    cookie::{Cookie, Key, SameSite},
};
use cookie::{CookieBuilder, time::Duration};
use oauth2::{
    AccessToken, AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointNotSet,
    EndpointSet, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, RevocationUrl, Scope,
    StandardRevocableToken, TokenResponse, TokenUrl, basic::BasicClient,
};
use poise::serenity_prelude::Http;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::{
    collections::{HashMap, HashSet},
    fmt::{Display, Formatter},
    sync::Arc,
};
use tokio::sync::RwLock;
use ultros_db::UltrosDb;

use super::error::{ApiError, WebError};

#[derive(Serialize, Deserialize, Debug, Eq, PartialOrd, PartialEq, Hash)]
pub enum OAuthScope {
    /// allows your app to fetch data from a user's "Now Playing/Recently Played" list - requires Discord approval
    #[serde(rename = "activities.read")]
    ActivitiesRead,
    /// allows your app to update a user's activity - requires Discord approval (NOT REQUIRED FOR GAMESDK ACTIVITY MANAGER)
    #[serde(rename = "activities.write")]
    ActivitiesWrite,
    //	allows your app to read build data for a user's applications
    #[serde(rename = "applications.builds.read")]
    ApplicationBuildsRead,
    //	allows your app to upload/update builds for a user's applications - requires Discord approval
    #[serde(rename = "applications.builds.upload")]
    ApplicationBuildsUpload,
    //	allows your app to use Slash Commands in a guild
    #[serde(rename = "applications.commands")]
    ApplicationsCommands,
    //	allows your app to update its Slash Commands via this bearer token - client credentials grant only
    #[serde(rename = "applications.commands.update")]
    ApplicationsCommandsUpdate,
    //	allows your app to read entitlements for a user's applications
    #[serde(rename = "applications.entitlements")]
    ApplicationsEntitlements,
    //	allows your app to read and update store data (SKUs, store listings, achievements, etc.) for a user's applications
    #[serde(rename = "applications.store.update")]
    ApplicationsStoreUpdate,
    //	for oauth2 bots this puts the bot in the user's selected guild by default
    #[serde(rename = "bot")]
    Bot,
    //	allows /users/@me/connections to return linked third-party accounts
    #[serde(rename = "connections")]
    Connections,
    // enables /users/@me to return an email
    #[serde(rename = "email")]
    Email,
    //	allows your app to join users to a group dm
    #[serde(rename = "gdm.join")]
    GroupDmJoin,
    // allows /users/@me/guilds to return basic information about all of a user's guilds
    #[serde(rename = "guilds")]
    Guilds,
    // allows /guilds/{guild.id}/members/{user.id} to be used for joining users to a guild
    #[serde(rename = "guilds.join")]
    GuildsJoin,
    // allows /users/@me without email
    #[serde(rename = "identify")]
    Identify,
    // for local rpc server api access, this allows you to read messages from all client channels (otherwise restricted to channels/guilds your app creates)
    #[serde(rename = "messages.read")]
    MessagesRead,
    //	allows your app to know a user's friends and implicit relationships - requires Discord approval
    #[serde(rename = "relationships.read")]
    RelationshipsRead,
    // for local rpc server access, this allows you to control a user's local Discord client - requires Discord approval
    #[serde(rename = "rpc")]
    Rpc,
    // for local rpc server access, this allows you to update a user's activity - requires Discord approval
    #[serde(rename = "rpc.activities.write")]
    RpcActivitiesWrite,
    // for local rpc server access, this allows you to receive notifications pushed out to the user - requires Discord approval
    #[serde(rename = "rpc.notifications.read")]
    RpcNotificationsRead,
    // for local rpc server access, this allows you to read a user's voice settings and listen for voice events - requires Discord approval
    #[serde(rename = "rpc.voice.read")]
    RpcVoiceRead,
    // for local rpc server access, this allows you to update a user's voice settings - requires Discord approval
    #[serde(rename = "rpc.voice.write")]
    RpcVoiceWrite,
    //	this generates a webhook that is returned in the oauth token response for authorization code grants
    #[serde(rename = "webhook.incoming")]
    WebhookIncoming,
}

impl Display for OAuthScope {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let s = match self {
            OAuthScope::ActivitiesRead => "activities.read",
            OAuthScope::ActivitiesWrite => "activities.write",
            OAuthScope::ApplicationBuildsRead => "applications.builds.read",
            OAuthScope::ApplicationBuildsUpload => "applications.builds.upload",
            OAuthScope::ApplicationsCommands => "applications.commands",
            OAuthScope::ApplicationsCommandsUpdate => "applications.commands.update",
            OAuthScope::ApplicationsEntitlements => "applications.entitlements",
            OAuthScope::ApplicationsStoreUpdate => "applications.store.update",
            OAuthScope::Bot => "bot",
            OAuthScope::Connections => "connections",
            OAuthScope::Email => "email",
            OAuthScope::GroupDmJoin => "gdm.join",
            OAuthScope::Guilds => "guilds",
            OAuthScope::GuildsJoin => "guilds.join",
            OAuthScope::Identify => "identify",
            OAuthScope::MessagesRead => "messages.read",
            OAuthScope::RelationshipsRead => "relationships.read",
            OAuthScope::Rpc => "rpc",
            OAuthScope::RpcActivitiesWrite => "rpc.activities.write",
            OAuthScope::RpcNotificationsRead => "rpc.notifications.read",
            OAuthScope::RpcVoiceRead => "rpc.voice.read",
            OAuthScope::RpcVoiceWrite => "rpc.voice.write",
            OAuthScope::WebhookIncoming => "webhook.incoming",
        };
        write!(f, "{}", s)
    }
}

const LOGIN_NEXT_COOKIE: &str = "login_next";

#[derive(Deserialize, Default)]
pub struct LoginParameters {
    next: Option<String>,
}

fn safe_login_next(next: Option<&str>) -> Option<String> {
    let next = next?.trim();
    if next.is_empty()
        || !next.starts_with('/')
        || next.starts_with("//")
        || next.contains('\\')
        || next.contains('\n')
        || next.contains('\r')
        || next.contains("://")
    {
        return None;
    }
    Some(next.to_string())
}

pub async fn begin_login(
    mut cookies: PrivateCookieJar,
    State(config): State<DiscordAuthConfig>,
    Query(params): Query<LoginParameters>,
) -> (PrivateCookieJar, Redirect) {
    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();

    if let Some(raw_next) = params.next.as_deref() {
        if let Some(next) = safe_login_next(Some(raw_next)) {
            cookies = cookies.add(
                CookieBuilder::new(LOGIN_NEXT_COOKIE, next)
                    .same_site(SameSite::Lax)
                    .secure(true)
                    .http_only(true)
                    .path("/")
                    .max_age(Duration::minutes(10))
                    .build(),
            );
        } else if let Some(cookie) = cookies.get(LOGIN_NEXT_COOKIE) {
            cookies = cookies.remove(cookie);
        }
    }

    let cookies = cookies.add(
        CookieBuilder::new("pkce_challenge", pkce_challenge.as_str().to_string())
            .same_site(SameSite::Strict)
            .secure(true)
            .http_only(true),
    );
    let cookies = cookies.add(
        CookieBuilder::new("pkce_verifier", pkce_verifier.secret().clone())
            .same_site(SameSite::Lax)
            .secure(true)
            .http_only(true)
            .build(),
    );

    let mut request = config
        .inner
        .client
        .authorize_url(CsrfToken::new_random)
        .set_pkce_challenge(pkce_challenge);
    for r in &config.inner.scopes {
        request = request.add_scope(Scope::new(r.to_string()));
    }
    let (url, csrf_token) = request.url();

    let cookies = cookies.add(
        CookieBuilder::new("oauth_state", csrf_token.secret().clone())
            .same_site(SameSite::Lax)
            .secure(true)
            .http_only(true)
            .build(),
    );

    (cookies, Redirect::to(url.as_str()))
}

#[derive(Deserialize)]
pub struct RedirectParameters {
    code: String,
    state: String,
}

pub async fn redirect(
    mut cookies: PrivateCookieJar,
    State(config): State<DiscordAuthConfig>,
    Query(RedirectParameters { code, state }): Query<RedirectParameters>,
) -> Result<(PrivateCookieJar, Redirect), WebError> {
    let code = AuthorizationCode::new(code);

    let saved_state = cookies.get("oauth_state").ok_or(WebError::BadRequest)?;
    if state != saved_state.value() {
        return Err(WebError::BadRequest);
    }
    cookies = cookies.remove(saved_state);

    if let Some(pkce_challenge) = cookies.get("pkce_challenge") {
        cookies = cookies.remove(pkce_challenge);
    }
    let pkce_verifier = if let Some(pkce_verifier) = cookies.get("pkce_verifier") {
        let secret = pkce_verifier.value().to_string();
        cookies = cookies.remove(pkce_verifier);
        secret
    } else {
        return Err(WebError::BadRequest);
    };
    let redirect_to = if let Some(login_next) = cookies.get(LOGIN_NEXT_COOKIE) {
        let redirect_to = safe_login_next(Some(login_next.value())).unwrap_or_else(|| "/".into());
        cookies = cookies.remove(login_next);
        redirect_to
    } else {
        "/".to_string()
    };
    let mut request = config.inner.client.exchange_code(code);
    request = request.set_pkce_verifier(PkceCodeVerifier::new(pkce_verifier));
    let token = request
        .request_async(&config.inner.http_client)
        .await?
        .access_token()
        .secret()
        .clone();
    // store the token into a cookie
    let mut cookie = Cookie::new("discord_auth", token);
    cookie.set_secure(true);
    cookie.set_same_site(SameSite::Lax);
    cookie.set_http_only(true);
    cookie.make_permanent();
    cookies = cookies.add(cookie);
    Ok((cookies, Redirect::to(&redirect_to)))
}

#[allow(clippy::collapsible_if)]
pub async fn logout(
    cookie_jar: PrivateCookieJar,
    State(config): State<DiscordAuthConfig>,
    State(cache): State<AuthUserCache>,
) -> Result<(PrivateCookieJar, Redirect), WebError> {
    let cookie = cookie_jar
        .get("discord_auth")
        .ok_or(WebError::NotAuthenticated)?;

    let token_value = cookie.value().to_string();
    cache.remove_token(&token_value).await;

    let token = AccessToken::new(token_value);
    // now try to revoke it async style
    if let Ok(revocable_token) = config
        .inner
        .client
        .revoke_token(StandardRevocableToken::AccessToken(token))
    {
        if let Err(e) = revocable_token
            .request_async(&config.inner.http_client)
            .await
        {
            tracing::warn!("Failed to revoke discord token on logout: {}", e);
        }
    }

    let cookie_jar = cookie_jar.remove(cookie);
    Ok((cookie_jar, Redirect::to("/")))
}

#[derive(Debug, Clone)]
pub struct AuthUserCache {
    users: Arc<RwLock<HashMap<String, AuthDiscordUser>>>,
}

impl AuthUserCache {
    pub fn new() -> Self {
        Self {
            users: Arc::default(),
        }
    }

    async fn store_user(&self, token: &str, user: AuthDiscordUser) {
        let mut users = self.users.write().await;
        users.insert(token.to_string(), user);
    }

    async fn get_user(&self, token: &str) -> Option<AuthDiscordUser> {
        let users = self.users.read().await;
        users.get(token).cloned()
    }

    pub(crate) async fn remove_token(&self, token: &str) {
        let mut users = self.users.write().await;
        users.remove(token);
    }
}

#[derive(Debug, Clone)]
pub struct AuthDiscordUser {
    pub(crate) id: u64,
    pub(crate) name: String,
    pub(crate) avatar_url: String,
}

impl<S> FromRequestParts<S> for AuthDiscordUser
where
    S: Send + Sync,
    axum_extra::extract::cookie::Key: FromRef<S>,
    UltrosDb: FromRef<S>,
    AuthUserCache: FromRef<S>,
{
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let cookie_jar: PrivateCookieJar<Key> = PrivateCookieJar::from_request_parts(parts, state)
            .await
            .unwrap();
        let discord_auth = cookie_jar
            .get("discord_auth")
            .ok_or(ApiError::NoAuthCookie)?;
        // get the discord user
        let State(ultros): State<UltrosDb> = State::from_request_parts(parts, state).await.unwrap();
        let State(user_cache): State<AuthUserCache> =
            State::from_request_parts(parts, state).await.unwrap();

        if let Some(user) = user_cache.get_user(discord_auth.value()).await {
            return Ok(user);
        }

        let http = Http::new(&format!("Bearer {}", discord_auth.value()));
        let user = http
            .get_current_user()
            .await
            .map_err(|_| ApiError::DiscordTokenInvalid(cookie_jar))?;
        let avatar_url = user
            .static_avatar_url()
            .unwrap_or_else(|| user.default_avatar_url());
        let user = AuthDiscordUser {
            id: user.id.get(),
            name: user.name.clone(),
            avatar_url,
        };
        ultros
            .get_or_create_discord_user(user.id, user.name.clone())
            .await?;
        user_cache
            .store_user(discord_auth.value(), user.clone())
            .await;
        Ok(user)
    }
}

#[derive(Clone)]
pub struct DiscordAuthConfig {
    inner: Arc<DiscordAuthConfigImpl>,
}

/// Concrete typestate for the Discord OAuth client: auth URI, token URI and
/// revocation URL are all configured; the device-auth and introspection URLs
/// are unused. The redirect URI is set on the client but isn't tracked by the
/// typestate generics.
type DiscordOAuthClient = BasicClient<
    EndpointSet,    // HasAuthUrl
    EndpointNotSet, // HasDeviceAuthUrl
    EndpointNotSet, // HasIntrospectionUrl
    EndpointSet,    // HasRevocationUrl
    EndpointSet,    // HasTokenUrl
>;

/// Provides authentication params
struct DiscordAuthConfigImpl {
    pub scopes: HashSet<OAuthScope>,
    pub client: DiscordOAuthClient,
    /// Shared async HTTP client for OAuth token exchange / revocation. v5
    /// requires us to bring our own `AsyncHttpClient` and configure it not to
    /// follow redirects to avoid SSRF on the token endpoint. We use the
    /// `reqwest` that oauth2 re-exports (0.12) rather than ultros's own
    /// `reqwest = 0.11`, because the `AsyncHttpClient` trait is only
    /// implemented for the 0.12 `reqwest::Client`.
    pub http_client: oauth2::reqwest::Client,
}

impl DiscordAuthConfig {
    pub fn new(
        client_id: String,
        client_secret: String,
        redirect_url: String,
        scopes: HashSet<OAuthScope>,
    ) -> Self {
        let client = BasicClient::new(ClientId::new(client_id))
            .set_client_secret(ClientSecret::new(client_secret))
            .set_auth_uri(
                AuthUrl::new("https://discord.com/api/oauth2/authorize".to_string())
                    .expect("Failed to parse url"),
            )
            .set_token_uri(
                TokenUrl::new("https://discord.com/api/oauth2/token".to_string())
                    .expect("Failed to parse token url"),
            )
            .set_redirect_uri(
                RedirectUrl::new(redirect_url.clone())
                    .unwrap_or_else(|_| panic!("Failed to parse redirect URL {}", redirect_url)),
            )
            .set_revocation_url(
                RevocationUrl::new("https://discord.com/api/oauth2/token/revoke".to_string())
                    .expect("Failed to parse revoke URL"),
            );
        let http_client = oauth2::reqwest::ClientBuilder::new()
            .timeout(std::time::Duration::from_secs(10))
            .redirect(oauth2::reqwest::redirect::Policy::none())
            .build()
            .expect("Failed to build oauth2 reqwest client");
        Self {
            inner: Arc::new(DiscordAuthConfigImpl {
                scopes,
                client,
                http_client,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::safe_login_next;

    #[test]
    fn safe_login_next_accepts_same_origin_relative_paths() {
        assert_eq!(
            safe_login_next(Some("/list/invite/abc123")),
            Some("/list/invite/abc123".to_string())
        );
        assert_eq!(
            safe_login_next(Some("  /settings?tab=account  ")),
            Some("/settings?tab=account".to_string())
        );
    }

    #[test]
    fn safe_login_next_rejects_external_or_malformed_targets() {
        assert_eq!(safe_login_next(None), None);
        assert_eq!(safe_login_next(Some("")), None);
        assert_eq!(safe_login_next(Some("https://evil.example")), None);
        assert_eq!(safe_login_next(Some("//evil.example/path")), None);
        assert_eq!(safe_login_next(Some("/\\evil")), None);
        assert_eq!(
            safe_login_next(Some("/path\r\nLocation: //evil.example")),
            None
        );
    }
}

// ---------------------------------------------------------------------------
// Test-only login bypass.
//
// Compile-time gated so the route can't accidentally ship to production:
// prod Docker builds don't pass `--features test-auth`, so this code isn't
// even in the binary. Local E2E + CI E2E builds opt in explicitly.
//
// Flow: caller hits `GET /test/login?user_id=...&username=...`, we
//   1. upsert the `discord_user` row,
//   2. seed an `AuthDiscordUser` into the in-memory `AuthUserCache` keyed by
//      a sentinel token like `test-token-<user_id>`,
//   3. set the `discord_auth` cookie to that token,
// so subsequent requests resolve via the cache and never touch Discord.
#[cfg(feature = "test-auth")]
pub mod test_auth {
    use super::{AuthDiscordUser, AuthUserCache};
    use axum::{
        extract::{Query, State},
        response::Redirect,
    };
    use axum_extra::extract::{
        PrivateCookieJar,
        cookie::{Cookie, SameSite},
    };
    use serde::Deserialize;
    use ultros_db::UltrosDb;

    use crate::web::error::WebError;

    #[derive(Deserialize)]
    pub struct TestLoginParams {
        pub user_id: u64,
        #[serde(default = "default_username")]
        pub username: String,
        #[serde(default = "default_redirect")]
        pub redirect: String,
    }

    fn default_username() -> String {
        "TestUser".to_string()
    }

    fn default_redirect() -> String {
        "/".to_string()
    }

    pub async fn test_login(
        cookies: PrivateCookieJar,
        State(db): State<UltrosDb>,
        State(cache): State<AuthUserCache>,
        Query(params): Query<TestLoginParams>,
    ) -> Result<(PrivateCookieJar, Redirect), WebError> {
        db.get_or_create_discord_user(params.user_id, params.username.clone())
            .await?;

        let token = format!("test-token-{}", params.user_id);
        let user = AuthDiscordUser {
            id: params.user_id,
            name: params.username,
            avatar_url: format!(
                "https://cdn.discordapp.com/embed/avatars/{}.png",
                params.user_id % 5
            ),
        };
        cache.store_user(&token, user).await;

        let mut cookie = Cookie::new("discord_auth", token);
        // Mirror oauth::redirect, but allow non-https so local E2E works.
        cookie.set_secure(false);
        cookie.set_same_site(SameSite::Lax);
        cookie.set_http_only(true);
        cookie.set_path("/");
        cookie.make_permanent();

        let redirect = if params.redirect.starts_with('/') {
            params.redirect.as_str()
        } else {
            "/"
        };
        Ok((cookies.add(cookie), Redirect::to(redirect)))
    }
}
