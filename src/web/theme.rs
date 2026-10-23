//! Light or dark appearance.
//!
//! Like on Apple devices, appearance belongs to the screen rather than to the
//! profile: it is kept in a cookie, so a phone can stay dark while a desktop
//! follows its system. "Automatic" (no cookie) follows the system setting.

use axum::http::{HeaderMap, HeaderValue, header};

use crate::i18n::Lang;

pub const COOKIE: &str = "korpus_theme";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Theme {
    #[default]
    Auto,
    Light,
    Dark,
}

impl Theme {
    pub const ALL: [Theme; 3] = [Theme::Auto, Theme::Light, Theme::Dark];

    pub fn as_str(self) -> &'static str {
        match self {
            Theme::Auto => "auto",
            Theme::Light => "light",
            Theme::Dark => "dark",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Theme::ALL.into_iter().find(|t| t.as_str() == s)
    }

    pub fn label(self, lang: Lang) -> &'static str {
        match self {
            Theme::Auto => lang.t("Automatic"),
            Theme::Light => lang.t("Light"),
            Theme::Dark => lang.t("Dark"),
        }
    }

    /// Value of `<meta name="color-scheme">`, so form controls and
    /// scrollbars match before the stylesheet loads.
    pub fn color_scheme(self) -> &'static str {
        match self {
            Theme::Auto => "light dark",
            Theme::Light => "light",
            Theme::Dark => "dark",
        }
    }

    /// Reads the cookie; anything unknown falls back to following the system.
    pub fn from_headers(headers: &HeaderMap) -> Theme {
        headers
            .get_all(header::COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(';'))
            .filter_map(|pair| pair.trim().split_once('='))
            .find(|(name, _)| *name == COOKIE)
            .and_then(|(_, value)| Theme::parse(value))
            .unwrap_or_default()
    }

    /// `Set-Cookie` value storing this choice for a year. It holds no
    /// personal data, so it is readable on plain HTTP installations too.
    pub fn set_cookie(self) -> HeaderValue {
        HeaderValue::from_static(match self {
            Theme::Auto => "korpus_theme=; Path=/; Max-Age=0; SameSite=Lax; HttpOnly",
            Theme::Light => "korpus_theme=light; Path=/; Max-Age=31536000; SameSite=Lax; HttpOnly",
            Theme::Dark => "korpus_theme=dark; Path=/; Max-Age=31536000; SameSite=Lax; HttpOnly",
        })
    }
}

tokio::task_local! {
    pub(super) static THEME: Theme;
}

/// The appearance of the request being rendered (automatic outside a
/// request). Templates call this so every page struct need not carry it.
pub fn current_theme() -> Theme {
    THEME.try_with(|t| *t).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(cookie: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(header::COOKIE, HeaderValue::from_str(cookie).unwrap());
        h
    }

    #[test]
    fn reads_theme_cookie() {
        assert_eq!(Theme::from_headers(&HeaderMap::new()), Theme::Auto);
        assert_eq!(
            Theme::from_headers(&headers("korpus_theme=dark")),
            Theme::Dark
        );
        assert_eq!(
            Theme::from_headers(&headers("a=1; korpus_theme=light; b=2")),
            Theme::Light
        );
        assert_eq!(
            Theme::from_headers(&headers("korpus_theme=neon")),
            Theme::Auto
        );
        assert_eq!(
            Theme::from_headers(&headers("xkorpus_theme=dark")),
            Theme::Auto
        );
    }

    #[test]
    fn cookie_round_trips() {
        for t in Theme::ALL {
            let set = t.set_cookie();
            let value = set.to_str().unwrap().split(';').next().unwrap();
            assert_eq!(Theme::from_headers(&headers(value)), t);
        }
    }
}
