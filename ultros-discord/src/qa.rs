//! Optional local QA scope. Invalid partial configuration must never fall
//! through to global command registration.
use crate::Error;

#[derive(Clone, Copy)]
pub(crate) struct QaScope {
    pub guild_id: u64,
    pub channel_id: u64,
}

impl QaScope {
    pub fn from_env() -> Result<Option<Self>, Error> {
        Self::parse(
            std::env::var("DISCORD_QA_GUILD_ID").ok().as_deref(),
            std::env::var("DISCORD_QA_CHANNEL_ID").ok().as_deref(),
        )
    }

    fn parse(guild: Option<&str>, channel: Option<&str>) -> Result<Option<Self>, Error> {
        if guild.is_none() && channel.is_none() {
            return Ok(None);
        }
        let parse =
            |value: Option<&str>| value.and_then(|v| v.parse::<u64>().ok()).filter(|v| *v > 0);
        match (parse(guild), parse(channel)) {
            (Some(guild_id), Some(channel_id)) => Ok(Some(Self {
                guild_id,
                channel_id,
            })),
            _ => Err(anyhow::anyhow!(
                "Set both DISCORD_QA_GUILD_ID and DISCORD_QA_CHANNEL_ID to valid Discord IDs"
            )
            .into()),
        }
    }

    pub fn allows(self, guild: Option<u64>, channel: u64) -> bool {
        guild == Some(self.guild_id) && channel == self.channel_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qa_registration_requires_both_valid_ids() {
        assert!(QaScope::parse(None, None).unwrap().is_none());
        for (guild, channel) in [
            (Some("1"), None),
            (None, Some("2")),
            (Some("0"), Some("2")),
            (Some("1"), Some("bad")),
            (Some(""), Some("2")),
        ] {
            assert!(QaScope::parse(guild, channel).is_err());
        }
        let scope = QaScope::parse(Some("1"), Some("2")).unwrap().unwrap();
        assert!(scope.allows(Some(1), 2));
        assert!(!scope.allows(Some(3), 2));
        assert!(!scope.allows(Some(1), 3));
        assert!(!scope.allows(None, 2));
    }
}
