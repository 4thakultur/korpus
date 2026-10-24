//! Configuration loaded from environment variables. See `.env.example`.

use std::net::{IpAddr, SocketAddr};
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
    /// Host names (besides IP addresses and `localhost`) that requests may
    /// be addressed to; `None` accepts any name.
    pub allowed_hosts: Option<Vec<String>>,
    /// Reverse proxies whose `X-Forwarded-For` names the real client, so
    /// failed logins are counted per client rather than per proxy.
    pub trusted_proxies: Vec<IpRange>,
}

/// One address, or a range in CIDR notation such as `172.16.0.0/12`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IpRange {
    addr: IpAddr,
    prefix: u8,
}

impl IpRange {
    pub fn parse(s: &str) -> Option<IpRange> {
        let (addr, prefix) = match s.split_once('/') {
            Some((a, p)) => (a.parse::<IpAddr>().ok()?, Some(p.parse::<u8>().ok()?)),
            None => (s.parse::<IpAddr>().ok()?, None),
        };
        let addr = addr.to_canonical();
        let max = if addr.is_ipv4() { 32 } else { 128 };
        let prefix = prefix.unwrap_or(max);
        (prefix <= max).then_some(IpRange { addr, prefix })
    }

    pub fn contains(&self, ip: IpAddr) -> bool {
        let shift = |bits: u32| bits.saturating_sub(u32::from(self.prefix));
        match (self.addr, ip.to_canonical()) {
            (IpAddr::V4(net), IpAddr::V4(ip)) => {
                u32::from(net).checked_shr(shift(32)).unwrap_or(0)
                    == u32::from(ip).checked_shr(shift(32)).unwrap_or(0)
            }
            (IpAddr::V6(net), IpAddr::V6(ip)) => {
                u128::from(net).checked_shr(shift(128)).unwrap_or(0)
                    == u128::from(ip).checked_shr(shift(128)).unwrap_or(0)
            }
            _ => false,
        }
    }
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

        let allowed_hosts = parse_hosts(&var("KORPUS_ALLOWED_HOSTS", ""))?;

        let mut trusted_proxies = Vec::new();
        for p in var("KORPUS_TRUSTED_PROXIES", "")
            .split(',')
            .map(str::trim)
            .filter(|p| !p.is_empty())
        {
            trusted_proxies.push(IpRange::parse(p).ok_or_else(|| ConfigError::Invalid {
                name: "KORPUS_TRUSTED_PROXIES",
                reason: format!(
                    "{p:?} is not an IP address or range (expected e.g. 127.0.0.1,172.16.0.0/12)"
                ),
            })?);
        }

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
            allowed_hosts,
            trusted_proxies,
        })
    }
}

/// `KORPUS_ALLOWED_HOSTS`: comma-separated host names, or `*` for any.
fn parse_hosts(v: &str) -> Result<Option<Vec<String>>, ConfigError> {
    if v.trim() == "*" {
        return Ok(None);
    }
    let mut hosts = Vec::new();
    for h in v.split(',').map(str::trim).filter(|h| !h.is_empty()) {
        let name = normalize_host(h);
        let valid = !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_'));
        if !valid {
            return Err(ConfigError::Invalid {
                name: "KORPUS_ALLOWED_HOSTS",
                reason: format!(
                    "{h:?} is not a host name (expected e.g. korpus.example.org,korpus.lan)"
                ),
            });
        }
        hosts.push(name);
    }
    Ok(Some(hosts))
}

/// Lower-cases a host name and drops a trailing dot, as browsers treat
/// `Example.org.` and `example.org` alike.
pub fn normalize_host(h: &str) -> String {
    h.trim_end_matches('.').to_ascii_lowercase()
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
        assert!(cfg(&[("KORPUS_ALLOWED_HOSTS", "https://korpus.lan")]).is_err());
        assert!(cfg(&[("KORPUS_ALLOWED_HOSTS", "korpus.lan:9874")]).is_err());
        assert!(cfg(&[("KORPUS_TRUSTED_PROXIES", "proxy.lan")]).is_err());
        assert!(cfg(&[("KORPUS_TRUSTED_PROXIES", "10.0.0.0/33")]).is_err());
    }

    #[test]
    fn trusted_proxy_ranges() {
        let c = cfg(&[(
            "KORPUS_TRUSTED_PROXIES",
            "127.0.0.1, 172.16.0.0/12,fd00::/8",
        )])
        .unwrap();
        let trusted = |ip: &str| {
            c.trusted_proxies
                .iter()
                .any(|r| r.contains(ip.parse().unwrap()))
        };
        for ip in ["127.0.0.1", "::ffff:127.0.0.1", "172.20.0.5", "fd12::1"] {
            assert!(trusted(ip), "{ip}");
        }
        for ip in ["127.0.0.2", "172.32.0.1", "192.168.1.1", "fe80::1", "::1"] {
            assert!(!trusted(ip), "{ip}");
        }
        assert!(
            IpRange::parse("0.0.0.0/0")
                .unwrap()
                .contains("8.8.8.8".parse().unwrap())
        );
        assert!(cfg(&[]).unwrap().trusted_proxies.is_empty());
    }

    #[test]
    fn allowed_hosts_default_to_none_but_local_names() {
        assert_eq!(cfg(&[]).unwrap().allowed_hosts, Some(vec![]));
        let c = cfg(&[("KORPUS_ALLOWED_HOSTS", " Korpus.Example.org., korpus.lan ,")]).unwrap();
        assert_eq!(
            c.allowed_hosts,
            Some(vec!["korpus.example.org".into(), "korpus.lan".into()])
        );
        assert_eq!(
            cfg(&[("KORPUS_ALLOWED_HOSTS", "*")]).unwrap().allowed_hosts,
            None
        );
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
