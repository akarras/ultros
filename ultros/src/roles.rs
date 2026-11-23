//! Runtime role selection (`ULTROS_ROLES`) for the single-binary two-role
//! split (`ultros-web` vs `ultros-market`).
//!
//! Leaving `ULTROS_ROLES` unset (or setting `all`) runs every subsystem in one
//! process — preserving existing local development, E2E, and single-container
//! production behavior with zero extra network hops.

use anyhow::{Result, bail};
use url::Url;

pub(crate) const ROLES_ENV: &str = "ULTROS_ROLES";
pub(crate) const MARKET_URL_ENV: &str = "ULTROS_MARKET_URL";
pub(crate) const MARKET_TOKEN_ENV: &str = "ULTROS_MARKET_TOKEN";

/// Active subsystems for this server process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Roles {
    pub(crate) web: bool,
    pub(crate) market: bool,
    pub(crate) ingest: bool,
    pub(crate) bot: bool,
}

impl Roles {
    pub(crate) const fn all() -> Self {
        Self {
            web: true,
            market: true,
            ingest: true,
            bot: true,
        }
    }

    pub(crate) fn from_env() -> Result<Self> {
        let raw = std::env::var(ROLES_ENV).ok();
        Self::parse(raw.as_deref())
    }

    pub(crate) fn parse(raw: Option<&str>) -> Result<Self> {
        let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
            return Ok(Self::all());
        };

        let mut roles = Self {
            web: false,
            market: false,
            ingest: false,
            bot: false,
        };

        for part in raw.split(',') {
            let token = part.trim().to_ascii_lowercase();
            match token.as_str() {
                "all" => roles = Self::all(),
                "web" => roles.web = true,
                "market" => roles.market = true,
                "ingest" => roles.ingest = true,
                "bot" => roles.bot = true,
                "" => bail!(
                    "invalid {ROLES_ENV}={raw:?}: empty role segment; expected a comma-separated list of all, web, market, ingest, bot"
                ),
                other => bail!(
                    "unknown role {other:?} in {ROLES_ENV}={raw:?}; expected one of: all, web, market, ingest, bot"
                ),
            }
        }

        if !roles.any() {
            bail!("{ROLES_ENV} enabled no roles");
        }

        Ok(roles)
    }

    pub(crate) const fn any(self) -> bool {
        self.web || self.market || self.ingest || self.bot
    }

    /// Whether this process must reach `ultros-market` over HTTP/2 instead of
    /// calling an in-process `AnalyzerService` / `RefreshJobs`.
    pub(crate) const fn uses_remote_market(self) -> bool {
        self.web && !self.market
    }

    /// Whether this process should run the dedicated internal HTTP/2 market
    /// API server (as opposed to mounting the full Leptos/OAuth web router).
    pub(crate) const fn runs_market_only_http(self) -> bool {
        self.market && !self.web
    }

    /// Ordered labels for logs and `/healthz`.
    pub(crate) fn active_names(self) -> Vec<&'static str> {
        let mut names = Vec::with_capacity(4);
        if self.web {
            names.push("web");
        }
        if self.market {
            names.push("market");
        }
        if self.ingest {
            names.push("ingest");
        }
        if self.bot {
            names.push("bot");
        }
        names
    }

    /// Validate role combinations and cross-role configuration at startup so
    /// misconfigured containers fail fast instead of serving broken traffic.
    pub(crate) fn validate(self, market_url: Option<&str>) -> Result<Option<Url>> {
        if self.bot && (!self.market || !self.ingest) {
            bail!(
                "{ROLES_ENV} includes 'bot', which currently requires both 'ingest' and 'market' in the same process (e.g. {ROLES_ENV}=ingest,market,bot)"
            );
        }

        if self.uses_remote_market() {
            let raw = market_url
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "{ROLES_ENV} includes 'web' without 'market'; set {MARKET_URL_ENV} (for example http://ultros-market:8081)"
                    )
                })?;
            let url = parse_market_url(raw)?;
            return Ok(Some(url));
        }

        Ok(None)
    }
}

/// Validate and normalize `ULTROS_MARKET_URL` (http/https only, host required,
/// no embedded credentials, query string, or fragment).
pub(crate) fn parse_market_url(raw: &str) -> Result<Url> {
    let trimmed = raw.trim();
    let mut url = Url::parse(trimmed)
        .map_err(|e| anyhow::anyhow!("invalid {MARKET_URL_ENV}={trimmed:?}: {e}"))?;
    match url.scheme() {
        "http" | "https" => {}
        scheme => {
            bail!(
                "invalid {MARKET_URL_ENV}: unsupported scheme {scheme:?} (expected http or https)"
            )
        }
    }
    if url.host().is_none() {
        bail!("invalid {MARKET_URL_ENV}: URL must include a host");
    }
    if !url.username().is_empty() || url.password().is_some() {
        bail!(
            "invalid {MARKET_URL_ENV}: embedded URL credentials are not allowed; use {MARKET_TOKEN_ENV} instead"
        );
    }
    if url.query().is_some() || url.fragment().is_some() {
        bail!("invalid {MARKET_URL_ENV}: query strings and fragments are not allowed");
    }
    // Strip trailing slashes on non-root paths so appending segments is stable.
    let trimmed_path = url.path().trim_end_matches('/').to_string();
    url.set_path(&trimmed_path);
    Ok(url)
}

/// Read the optional internal shared bearer token for `/internal/market/*` and
/// `/internal/bot/*`.
pub(crate) fn market_token_from_env() -> Option<String> {
    std::env::var(MARKET_TOKEN_ENV)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unset_or_blank_defaults_to_all() {
        assert_eq!(Roles::parse(None).unwrap(), Roles::all());
        assert_eq!(Roles::parse(Some("")).unwrap(), Roles::all());
        assert_eq!(Roles::parse(Some("   ")).unwrap(), Roles::all());
        assert_eq!(Roles::parse(Some("all")).unwrap(), Roles::all());
        assert_eq!(Roles::parse(Some(" ALL ")).unwrap(), Roles::all());
    }

    #[test]
    fn parses_two_role_split_combinations() {
        let web = Roles::parse(Some("web")).unwrap();
        assert_eq!(
            web,
            Roles {
                web: true,
                market: false,
                ingest: false,
                bot: false,
            }
        );
        assert!(web.uses_remote_market());
        assert!(!web.runs_market_only_http());

        let worker = Roles::parse(Some("ingest, market, bot")).unwrap();
        assert_eq!(
            worker,
            Roles {
                web: false,
                market: true,
                ingest: true,
                bot: true,
            }
        );
        assert!(!worker.uses_remote_market());
        assert!(worker.runs_market_only_http());
        assert_eq!(worker.active_names(), vec!["market", "ingest", "bot"]);
    }

    #[test]
    fn rejects_unknown_or_empty_segments() {
        assert!(Roles::parse(Some("web,unknown")).is_err());
        assert!(Roles::parse(Some("web,")).is_err());
        assert!(Roles::parse(Some(",market")).is_err());
        assert!(Roles::parse(Some("web,,market")).is_err());
    }

    #[test]
    fn validates_role_dependencies_and_market_url() {
        let web = Roles::parse(Some("web")).unwrap();
        assert!(web.validate(None).is_err());
        assert!(web.validate(Some("   ")).is_err());
        assert!(web.validate(Some("ftp://ultros-market:8081")).is_err());
        assert!(
            web.validate(Some("http://user:pass@ultros-market:8081"))
                .is_err()
        );
        assert!(
            web.validate(Some("http://ultros-market:8081?foo=bar"))
                .is_err()
        );
        let url = web
            .validate(Some("http://ultros-market:8081/"))
            .unwrap()
            .expect("remote market url");
        assert_eq!(url.as_str(), "http://ultros-market:8081/");

        let prefixed = web
            .validate(Some("https://market.internal/api-root/"))
            .unwrap()
            .expect("prefixed market url");
        assert_eq!(prefixed.as_str(), "https://market.internal/api-root");

        // bot currently requires both ingest and market.
        assert!(Roles::parse(Some("bot")).unwrap().validate(None).is_err());
        assert!(
            Roles::parse(Some("market,bot"))
                .unwrap()
                .validate(None)
                .is_err()
        );
        assert!(
            Roles::parse(Some("ingest,market,bot"))
                .unwrap()
                .validate(Some("http://ultros-market:8081"))
                .unwrap()
                .is_none()
        );

        // `all` ignores ULTROS_MARKET_URL so a shared .env never causes self-recursion.
        assert!(
            Roles::all()
                .validate(Some("http://ultros-market:8081"))
                .unwrap()
                .is_none()
        );
    }
}
