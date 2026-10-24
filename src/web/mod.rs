//! HTTP routing, middleware and shared handler helpers.

mod diary;
mod foods;
mod goals;
mod search;
mod settings;
mod theme;
mod weight;

use std::sync::Arc;
use std::time::Duration;

use askama::Template;
use axum::Router;
use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::{HeaderName, HeaderValue, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use jiff::civil::Date;
use serde::Deserialize;
use sqlx::SqlitePool;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

use crate::config::Config;
use crate::diary::Meal;
use crate::error::{AppError, AppResult};
use crate::i18n::Lang;
use crate::providers::Providers;

pub use theme::{Theme, current_theme};

#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    pub providers: Arc<Providers>,
    pub config: Arc<Config>,
}

/// Default request body limit; forms are small.
const BODY_LIMIT: usize = 64 * 1024;

pub fn router(state: AppState) -> Router {
    let csp = "default-src 'self'; img-src 'self' data:; style-src 'self'; script-src 'self'; \
               connect-src 'self'; form-action 'self'; frame-ancestors 'none'; base-uri 'none'; \
               object-src 'none'";

    let import = Router::new()
        .route("/settings/import", post(settings::import))
        .layer(DefaultBodyLimit::disable())
        .layer(RequestBodyLimitLayer::new(
            crate::backup::MAX_IMPORT_BYTES + 64 * 1024,
        ));

    let dataset_upload = Router::new()
        .route("/settings/datasets/{id}", post(settings::import_dataset))
        .layer(DefaultBodyLimit::disable())
        .layer(RequestBodyLimitLayer::new(
            crate::datasets::MAX_UPLOAD_BYTES + 64 * 1024,
        ));

    Router::new()
        .route("/", get(diary::root))
        .route("/setup", get(settings::setup_form).post(settings::setup_save))
        .route("/diary", get(diary::day))
        .route("/diary/entries", post(diary::create))
        .route("/diary/entries/{id}/edit", get(diary::edit_form))
        .route("/diary/entries/{id}", post(diary::update))
        .route("/diary/entries/{id}/delete", post(diary::delete))
        .route("/search", get(search::page))
        .route("/search/remote", get(search::remote_fragment))
        .route("/search/history/clear", post(search::clear_history))
        .route("/foods", get(|| async { Redirect::to("/search") }).post(foods::create))
        .route("/foods/new", get(foods::new_form))
        .route("/foods/import", post(foods::import))
        .route("/foods/{id}", get(foods::detail).post(foods::update))
        .route("/foods/{id}/edit", get(foods::edit_form))
        .route("/foods/{id}/delete", get(foods::delete_confirm).post(foods::delete))
        .route("/weight", get(weight::page).post(weight::create))
        .route("/weight/{id}/edit", get(weight::edit_form))
        .route("/weight/{id}", post(weight::update))
        .route("/weight/{id}/delete", post(weight::delete))
        .route("/goals", get(goals::page).post(goals::save))
        .route("/settings", get(settings::page).post(settings::save))
        .route("/settings/appearance", post(settings::appearance))
        .route("/settings/export.json", get(settings::export_json))
        .route("/settings/diary.csv", get(settings::export_csv))
        .route("/settings/backup.sqlite", get(settings::backup))
        .route("/settings/datasets/{id}/remove", post(settings::remove_dataset))
        .route("/static/app.css", get(static_css))
        .route("/static/app.js", get(static_js))
        .route("/healthz", get(|| async { "ok" }))
        .route("/readyz", get(readyz))
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .merge(import)
        .merge(dataset_upload)
        .fallback(|| async { AppError::NotFound })
        .layer(middleware::from_fn(same_origin_writes))
        .layer(middleware::from_fn_with_state(state.clone(), language))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(csp),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::REFERRER_POLICY,
            HeaderValue::from_static("same-origin"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::X_FRAME_OPTIONS,
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            HeaderName::from_static("permissions-policy"),
            HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-store"),
        ))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::SERVICE_UNAVAILABLE,
            Duration::from_secs(30),
        ))
        .layer(
            // Log method and path only: query strings can contain food searches.
            TraceLayer::new_for_http().make_span_with(|req: &Request| {
                tracing::info_span!("request", method = %req.method(), path = %req.uri().path())
            }),
        )
        .with_state(state)
}

tokio::task_local! {
    static LANG: Lang;
}

/// The interface language of the request being handled (English outside a
/// request).
pub fn current_lang() -> Lang {
    LANG.try_with(|l| *l).unwrap_or_default()
}

/// Resolves the interface language once per request: the profile setting,
/// or the browser's `Accept-Language` when none is chosen. Handlers and
/// error pages read it with [`current_lang`]. The appearance cookie is
/// resolved alongside it for [`current_theme`].
async fn language(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let path = req.uri().path();
    if path.starts_with("/static/") || path == "/healthz" || path == "/readyz" {
        return next.run(req).await;
    }
    let chosen = match crate::profile::language(&state.db).await {
        Ok(l) => l,
        Err(e) => {
            tracing::warn!(error = %e, "could not read language setting");
            None
        }
    };
    let lang = chosen.unwrap_or_else(|| browser_lang(req.headers()));
    let theme = Theme::from_headers(req.headers());
    LANG.scope(lang, theme::THEME.scope(theme, next.run(req)))
        .await
}

pub fn browser_lang(headers: &axum::http::HeaderMap) -> Lang {
    headers
        .get(header::ACCEPT_LANGUAGE)
        .and_then(|v| v.to_str().ok())
        .map(Lang::from_accept_language)
        .unwrap_or_default()
}

/// CSRF protection for cookie-less, same-origin form posts: browsers send
/// `Sec-Fetch-Site` and/or `Origin` on cross-site requests, which are
/// rejected for every state-changing method.
async fn same_origin_writes(req: Request, next: Next) -> Response {
    if matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS) {
        return next.run(req).await;
    }
    let headers = req.headers();
    if let Some(site) = headers.get("sec-fetch-site").and_then(|v| v.to_str().ok())
        && !matches!(site, "same-origin" | "none")
    {
        return AppError::Forbidden(
            current_lang()
                .t("Cross-site form submissions are not allowed.")
                .into(),
        )
        .into_response();
    }
    if let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        let header_str = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
        let origin_host = origin
            .strip_prefix("https://")
            .or_else(|| origin.strip_prefix("http://"));
        // Reverse proxies may rewrite Host; accept the forwarded host they report.
        let matches = origin_host.is_some()
            && (origin_host == header_str("host") || origin_host == header_str("x-forwarded-host"));
        if !matches {
            return AppError::Forbidden(
                current_lang()
                    .t("Cross-origin form submissions are not allowed.")
                    .into(),
            )
            .into_response();
        }
    }
    next.run(req).await
}

async fn readyz(State(state): State<AppState>) -> Response {
    match sqlx::query_scalar::<_, i64>("SELECT 1")
        .fetch_one(&state.db)
        .await
    {
        Ok(_) => "ready".into_response(),
        Err(e) => {
            tracing::error!(error = %e, "readiness check failed");
            (StatusCode::SERVICE_UNAVAILABLE, "database unavailable").into_response()
        }
    }
}

fn static_asset(content_type: &'static str, body: &'static str) -> Response {
    (
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        body,
    )
        .into_response()
}

async fn static_css() -> Response {
    static_asset("text/css; charset=utf-8", APP_CSS)
}

async fn static_js() -> Response {
    static_asset("text/javascript; charset=utf-8", APP_JS)
}

const APP_CSS: &str = include_str!("../../static/app.css");
const APP_JS: &str = include_str!("../../static/app.js");

/// Asset version appended to static URLs: a hash of their contents, so
/// browsers refetch whenever the CSS or script changes, even within one
/// release (they are cached for a day).
pub const ASSET_VERSION: u64 = fnv1a(
    APP_JS.as_bytes(),
    fnv1a(APP_CSS.as_bytes(), 0xcbf2_9ce4_8422_2325),
);

const fn fnv1a(bytes: &[u8], mut hash: u64) -> u64 {
    let mut i = 0;
    while i < bytes.len() {
        hash ^= bytes[i] as u64;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
        i += 1;
    }
    hash
}

pub fn render<T: Template>(t: &T) -> AppResult<Html<String>> {
    Ok(Html(t.render()?))
}

pub fn render_status<T: Template>(status: StatusCode, t: &T) -> AppResult<Response> {
    Ok((status, Html(t.render()?)).into_response())
}

/// Post/redirect/get with an optional fixed status message.
pub fn see_other(url: &str) -> Response {
    Redirect::to(url).into_response()
}

/// Status messages shown after a redirect. Only known keys are displayed, so
/// arbitrary text can never be injected through the URL.
pub fn flash_message(key: Option<&str>, lang: Lang) -> Option<&'static str> {
    Some(match key? {
        "added" => lang.t("Added to your diary."),
        "entry_saved" => lang.t("Entry updated."),
        "entry_deleted" => lang.t("Entry deleted."),
        "food_created" => lang.t("Food created."),
        "food_saved" => lang.t("Food saved. Existing diary entries keep their logged values."),
        "food_deleted" => lang.t("Food deleted. Existing diary entries keep their logged values."),
        "imported_food" => lang.t("Product saved to your foods."),
        "history_cleared" => lang.t("History cleared. Your diary and saved foods are unchanged."),
        "weight_saved" => lang.t("Weight recorded."),
        "weight_replaced" => lang.t("Weight for that date updated."),
        "weight_deleted" => lang.t("Weight entry deleted."),
        "goals_saved" => lang.t("Goal settings saved."),
        "settings_saved" => lang.t("Settings saved."),
        "appearance_saved" => lang.t("Appearance saved on this device."),
        "imported" => lang.t("Import complete."),
        "dataset_imported" => {
            lang.t("Food table imported. Search now uses it where it fits your country.")
        }
        "dataset_removed" => {
            lang.t("Food table removed. Foods you already saved from it are kept.")
        }
        _ => return None,
    })
}

/// Diary context carried through search and food pages so that a food picked
/// from "Add food" lands in the right date and meal.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct LogContextQuery {
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub meal: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LogContext {
    pub date: Option<Date>,
    pub meal: Option<Meal>,
}

impl LogContext {
    pub fn from_parts(date: Option<&str>, meal: Option<&str>) -> Self {
        LogContext {
            date: date.and_then(crate::units::parse_date),
            meal: meal.and_then(Meal::parse),
        }
    }

    pub fn is_logging(&self) -> bool {
        self.date.is_some() || self.meal.is_some()
    }

    /// Query string (without leading `?`), only containing validated values.
    pub fn query(&self) -> String {
        let mut parts = Vec::new();
        if let Some(d) = self.date {
            parts.push(format!("date={d}"));
        }
        if let Some(m) = self.meal {
            parts.push(format!("meal={}", m.as_str()));
        }
        parts.join("&")
    }

    pub fn date_str(&self) -> String {
        self.date.map(|d| d.to_string()).unwrap_or_default()
    }

    pub fn meal_str(&self) -> &'static str {
        self.meal.map(Meal::as_str).unwrap_or("")
    }

    pub fn date_label(&self, lang: Lang) -> String {
        self.date
            .map(|d| crate::units::fmt_date_short(d, lang))
            .unwrap_or_default()
    }

    pub fn meal_label(&self, lang: Lang) -> &'static str {
        self.meal.map(|m| m.label(lang)).unwrap_or("")
    }

    /// Appends the context to a URL path that has no query string yet.
    pub fn url(&self, path: &str) -> String {
        let q = self.query();
        if q.is_empty() {
            path.to_string()
        } else {
            format!("{path}?{q}")
        }
    }
}

pub fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub fn parse_id(s: &str) -> AppResult<i64> {
    s.parse::<i64>()
        .ok()
        .filter(|id| *id > 0)
        .ok_or(AppError::NotFound)
}

/// Whether the browser asked for an HTML fragment (progressive enhancement).
pub fn wants_fragment(headers: &axum::http::HeaderMap) -> bool {
    headers
        .get("x-requested-with")
        .and_then(|v| v.to_str().ok())
        == Some("fetch")
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    #[test]
    fn log_context_only_keeps_valid_values() {
        let c = LogContext::from_parts(Some("2026-10-22"), Some("lunch"));
        assert_eq!(c.query(), "date=2026-10-22&meal=lunch");
        assert_eq!(c.url("/search"), "/search?date=2026-10-22&meal=lunch");
        let bad = LogContext::from_parts(Some("<script>"), Some("brunch"));
        assert_eq!(bad, LogContext::default());
        assert_eq!(bad.url("/search"), "/search");
        assert_eq!(
            LogContext::from_parts(Some("2026-01-02"), None).date,
            Some(date(2026, 1, 2))
        );
    }

    #[test]
    fn encodes_query_values() {
        assert_eq!(
            url_encode("crème fraîche&x=1"),
            "cr%C3%A8me+fra%C3%AEche%26x%3D1"
        );
    }

    #[test]
    fn unknown_flash_keys_are_ignored() {
        assert!(flash_message(Some("added"), Lang::En).is_some());
        assert_eq!(flash_message(Some("<b>hi</b>"), Lang::En), None);
        assert_eq!(flash_message(None, Lang::En), None);
    }
}
