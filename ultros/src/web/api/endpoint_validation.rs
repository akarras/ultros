use crate::web::error::ApiError;

#[allow(clippy::result_large_err)]
pub(crate) fn validate_discord_webhook_url(url: &str) -> Result<(), ApiError> {
    let parsed = url::Url::parse(url)
        .map_err(|e| ApiError::from(anyhow::anyhow!("invalid webhook URL: {e}")))?;
    if parsed.scheme() != "https" {
        return Err(ApiError::from(anyhow::anyhow!(
            "webhook URL must use https"
        )));
    }
    let host = parsed.host_str().unwrap_or("");
    let allowed = [
        "discord.com",
        "discordapp.com",
        "ptb.discord.com",
        "canary.discord.com",
    ];
    if !allowed.contains(&host) {
        return Err(ApiError::from(anyhow::anyhow!(
            "webhook URL host must be a Discord webhook host"
        )));
    }
    if !parsed.path().starts_with("/api/webhooks/") {
        return Err(ApiError::from(anyhow::anyhow!(
            "webhook URL path must start with /api/webhooks/"
        )));
    }
    Ok(())
}

#[allow(clippy::result_large_err, dead_code)]
pub(crate) fn validate_discord_channel_id(channel_id: i64) -> Result<(), ApiError> {
    if channel_id <= 0 {
        return Err(ApiError::from(anyhow::anyhow!(
            "channel_id must be positive"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- validate_discord_webhook_url ----------

    #[test]
    fn webhook_url_accepts_canonical_discord_host() {
        assert!(validate_discord_webhook_url("https://discord.com/api/webhooks/1/abc").is_ok());
    }

    #[test]
    fn webhook_url_accepts_all_documented_discord_hosts() {
        for host in [
            "discord.com",
            "discordapp.com",
            "ptb.discord.com",
            "canary.discord.com",
        ] {
            let url = format!("https://{host}/api/webhooks/1/abc");
            assert!(
                validate_discord_webhook_url(&url).is_ok(),
                "expected ok for {url}"
            );
        }
    }

    #[test]
    fn webhook_url_rejects_non_https_scheme() {
        assert!(validate_discord_webhook_url("http://discord.com/api/webhooks/1/abc").is_err());
        assert!(validate_discord_webhook_url("ftp://discord.com/api/webhooks/1/abc").is_err());
    }

    #[test]
    fn webhook_url_rejects_non_discord_host() {
        assert!(validate_discord_webhook_url("https://evil.com/api/webhooks/1/abc").is_err());
        assert!(
            validate_discord_webhook_url("https://discord.com.evil.com/api/webhooks/1/abc")
                .is_err()
        );
    }

    #[test]
    fn webhook_url_rejects_wrong_path_prefix() {
        assert!(validate_discord_webhook_url("https://discord.com/").is_err());
        assert!(validate_discord_webhook_url("https://discord.com/api/").is_err());
        assert!(
            validate_discord_webhook_url("https://discord.com/login?next=/api/webhooks/").is_err()
        );
    }

    #[test]
    fn webhook_url_rejects_garbage_string() {
        assert!(validate_discord_webhook_url("not a url").is_err());
        assert!(validate_discord_webhook_url("").is_err());
    }
}
