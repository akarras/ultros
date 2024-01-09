use async_trait::async_trait;
use axum::{
    extract::{FromRef, FromRequestParts, Query, State},
    http::request::Parts,
    response::Redirect,
};
use axum_extra::extract::{
    cookie::{Cookie, Key, SameSite},
    PrivateCookieJar,
};
use oauth2::{
    basic::BasicClient, AccessToken, AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken,
    PkceCodeChallenge, RedirectUrl, RevocationUrl, Scope, StandardRevocableToken, TokenResponse,
    TokenUrl,
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
        let renamed_value = serde_json::to_string(self).expect("Should never fail this serialize");
        // this is a bad hack, I should have thought of something better than serde rename to get this display
        write!(f, "{}", renamed_value.replace('"', ""))
    }
}

pub async fn begin_login(
    cookies: PrivateCookieJar,
    State(config): State<DiscordAuthConfig>,
) -> (PrivateCookieJar, Redirect) {
    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
    // todo: send redirect handler for discord

    let cookies = cookies.add(
        Cookie::build("pkce_challenge", pkce_challenge.as_str().to_string())
            .same_site(SameSite::Strict)
            .secure(true)
            .finish(),
    );
    let cookies = cookies.add(Cookie::new("pkce_verifier", pkce_verifier.secret().clone()));

    let mut request = config.inner.client.authorize_url(CsrfToken::new_random);
    for r in &config.inner.scopes {
        request = request.add_scope(Scope::new(r.to_string()));
    }
    let (url, _token) = request.url();

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
    let _state = CsrfToken::new(state);
    if let Some(pkce_challenge) = cookies.get("pkce_challenge") {
        cookies = cookies.remove(pkce_challenge);
    }
    if let Some(pkce_verifier) = cookies.get("pkce_verifier") {
        cookies = cookies.remove(pkce_verifier);
    }
    let token = config
        .inner
        .client
        .exchange_code(code)
        .request_async(oauth2::reqwest::async_http_client)
        .await?
        .access_token()
        .secret()
        .clone();
    // store the token into a cookie
    let mut cookie = Cookie::new("discord_auth", token);
    cookie.set_secure(true);
    cookie.set_same_site(SameSite::Lax);
    cookie.make_permanent();
    cookies = cookies.add(cookie);
    Ok((cookies, Redirect::to("/")))
}

pub async fn logout(
    cookie_jar: PrivateCookieJar,
    State(config): State<DiscordAuthConfig>,
) -> Result<(PrivateCookieJar, Redirect), WebError> {
    let cookie = cookie_jar
        .get("discord_auth")
        .ok_or(WebError::NotAuthenticated)?;
    let token = AccessToken::new(cookie.value().to_string());
    // now try to revoke it async style
    config
        .inner
        .client
        .revoke_token(StandardRevocableToken::AccessToken(token))?
        .request_async(oauth2::reqwest::async_http_client)
        .await?;
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

#[async_trait]
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
            id: user.id.0,
            name: user.name,
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

/// Provides authentication params
#[derive(Debug)]
struct DiscordAuthConfigImpl {
    pub scopes: HashSet<OAuthScope>,
    pub client: BasicClient,
}

impl DiscordAuthConfig {
    pub fn new(
        client_id: String,
        client_secret: String,
        redirect_url: String,
        scopes: HashSet<OAuthScope>,
    ) -> Self {
        let client = BasicClient::new(
            ClientId::new(client_id),
            Some(ClientSecret::new(client_secret)),
            AuthUrl::new("https://discord.com/api/oauth2/authorize".to_string())
                .expect("Failed to parse url"),
            Some(
                TokenUrl::new("https://discord.com/api/oauth2/token".to_string())
                    .expect("Failed to parse token url"),
            ),
        )
        .set_redirect_uri(
            RedirectUrl::new(redirect_url.clone())
                .unwrap_or_else(|_| panic!("Failed to parse redirect URL {}", redirect_url)),
        )
        .set_revocation_uri(
            RevocationUrl::new("https://discord.com/api/oauth2/token/revoke".to_string())
                .expect("Failed to parse revoke URL"),
        );
        Self {
            inner: Arc::new(DiscordAuthConfigImpl { scopes, client }),
        }
    }
}
