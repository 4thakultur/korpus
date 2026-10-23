//! Configuration loaded from environment variables. See `.env.example`.

use std::net::SocketAddr;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct Config {
    pub bind: SocketAddr,
    pub database_url: String,
    /// Time zone used for "today" until the profile sets one.
    pub default_timezone: String,
    pub off_enabled: bool,
    pub off_base_url: String,
    pub off_search_url: String,
    pub provider_timeout: Duration,
    pub user_agent: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("{name} is invalid: {reason}")]
    Invalid { name: &'static str, reason: String },
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|k| std::env::var(k).ok())
    }

    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let var = |k: &str, default: &str| get(k).unwrap_or_else(|| default.to_string());

        let bind_s = var("KORPUS_BIND", "127.0.0.1:9874");
        let bind = bind_s.parse().map_err(|e| ConfigError::Invalid {
            name: "KORPUS_BIND",
            reason: format!("{e} (expected e.g. 127.0.0.1:9874)"),
        })?;

        let default_timezone = var("KORPUS_DEFAULT_TIMEZONE", "UTC");
        jiff::tz::TimeZone::get(&default_timezone).map_err(|e| ConfigError::Invalid {
            name: "KORPUS_DEFAULT_TIMEZONE",
            reason: e.to_string(),
        })?;

        let off_enabled = parse_bool("KORPUS_OFF_ENABLED", &var("KORPUS_OFF_ENABLED", "true"))?;

        let timeout_s = var("KORPUS_PROVIDER_TIMEOUT_SECS", "8");
        let timeout_secs: u64 = timeout_s
            .parse()
            .ok()
            .filter(|s| (1..=60).contains(s))
            .ok_or(ConfigError::Invalid {
                name: "KORPUS_PROVIDER_TIMEOUT_SECS",
                reason: "expected a whole number of seconds between 1 and 60".into(),
            })?;

        let contact = get("KORPUS_CONTACT").filter(|c| !c.trim().is_empty());
        let user_agent = match contact {
            Some(c) => format!("Korpus/{} ({})", env!("CARGO_PKG_VERSION"), c.trim()),
            None => format!("Korpus/{} (self-hosted)", env!("CARGO_PKG_VERSION")),
        };

        Ok(Config {
            bind,
            database_url: var("KORPUS_DATABASE_URL", "sqlite://data/korpus.db"),
            default_timezone,
            off_enabled,
            off_base_url: var("KORPUS_OFF_BASE_URL", "https://world.openfoodfacts.org")
                .trim_end_matches('/')
                .to_string(),
            off_search_url: var("KORPUS_OFF_SEARCH_URL", "https://search.openfoodfacts.org")
                .trim_end_matches('/')
                .to_string(),
            provider_timeout: Duration::from_secs(timeout_secs),
            user_agent,
        })
    }
}

fn parse_bool(name: &'static str, v: &str) -> Result<bool, ConfigError> {
    match v.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(ConfigError::Invalid {
            name,
            reason: "expected true or false".into(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn cfg(pairs: &[(&str, &str)]) -> Result<Config, ConfigError> {
        let m: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Config::from_lookup(|k| m.get(k).cloned())
    }

    #[test]
    fn defaults_are_local_only_bind() {
        let c = cfg(&[]).unwrap();
        assert_eq!(c.bind.to_string(), "127.0.0.1:9874");
        assert!(c.off_enabled);
        assert_eq!(c.default_timezone, "UTC");
    }

    #[test]
    fn rejects_invalid_values() {
        assert!(cfg(&[("KORPUS_BIND", "nope")]).is_err());
        assert!(cfg(&[("KORPUS_OFF_ENABLED", "maybe")]).is_err());
        assert!(cfg(&[("KORPUS_PROVIDER_TIMEOUT_SECS", "0")]).is_err());
        assert!(cfg(&[("KORPUS_DEFAULT_TIMEZONE", "Mars/Olympus")]).is_err());
    }

    #[test]
    fn provider_can_be_disabled_and_contact_set() {
        let c = cfg(&[
            ("KORPUS_OFF_ENABLED", "false"),
            ("KORPUS_CONTACT", "me@example.org"),
        ])
        .unwrap();
        assert!(!c.off_enabled);
        assert!(c.user_agent.contains("me@example.org"));
    }
}
