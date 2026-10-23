//! Food search across local foods and every enabled provider, shown as one
//! list: saved foods first, then provider products not saved yet.
//!
//! Local results render immediately. Provider results are fetched in a
//! second request and appended (or fetched inline when the form is
//! submitted without the page script), so a slow or unreachable provider
//! never delays local search.

use askama::Template;
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use super::{
    AppState, LogContext, current_lang, flash_message, render, url_encode, wants_fragment,
};
use crate::error::AppResult;
use crate::foods::{self, Food, MAX_QUERY_CHARS, is_valid_barcode, normalize_barcode};
use crate::i18n::Lang;
use crate::nutrition::{Nutrients, fmt_opt_g, fmt_opt_kcal};
use crate::providers::{ProviderError, ProviderFood, ProviderPage};

const MAX_PAGE: usize = 50;

#[derive(Deserialize, Default)]
pub struct SearchQuery {
    #[serde(default)]
    q: String,
    date: Option<String>,
    meal: Option<String>,
    page: Option<usize>,
    /// Render provider results inline (no-JavaScript fallback).
    remote: Option<String>,
    msg: Option<String>,
}

impl SearchQuery {
    fn query(&self) -> String {
        self.q.trim().chars().take(MAX_QUERY_CHARS).collect()
    }
    fn page(&self) -> usize {
        self.page.unwrap_or(1).clamp(1, MAX_PAGE)
    }
    fn ctx(&self) -> LogContext {
        LogContext::from_parts(self.date.as_deref(), self.meal.as_deref())
    }
}

pub struct ResultRow {
    pub name: String,
    pub brand: String,
    pub basis_label: &'static str,
    pub kcal: String,
    pub protein: String,
    pub carbs: String,
    pub fat: String,
    pub missing: String,
}

fn missing_label(n: &Nutrients, lang: Lang) -> String {
    let missing: Vec<&str> = n
        .missing_core()
        .into_iter()
        .map(|k| k.label(lang))
        .collect();
    missing.join(", ")
}

/// The list shows only what helps pick a food; source, barcode and dates
/// are on the food's own page.
fn row(
    name: &str,
    brand: Option<&str>,
    basis: crate::nutrition::Basis,
    n: &Nutrients,
    lang: Lang,
) -> ResultRow {
    ResultRow {
        name: name.to_string(),
        brand: brand.unwrap_or("").to_string(),
        basis_label: basis.label(lang),
        kcal: fmt_opt_kcal(n.energy_kcal),
        protein: lang.num(fmt_opt_g(n.protein_g)),
        carbs: lang.num(fmt_opt_g(n.carbs_g)),
        fat: lang.num(fmt_opt_g(n.fat_g)),
        missing: missing_label(n, lang),
    }
}

/// One entry in the single result list: a saved food (opened by link) or a
/// provider product (saved locally when selected).
pub struct ResultItem {
    /// Detail page of a saved food.
    pub url: Option<String>,
    /// Provider record to import, for products not saved yet.
    pub source: &'static str,
    pub provider_id: String,
    pub row: ResultRow,
}

fn local_item(f: &Food, ctx: &LogContext, lang: Lang) -> ResultItem {
    ResultItem {
        url: Some(ctx.url(&format!("/foods/{}", f.id))),
        source: "",
        provider_id: String::new(),
        row: row(&f.name, f.brand.as_deref(), f.basis(), &f.nutrients(), lang),
    }
}

fn remote_item(p: &ProviderFood, lang: Lang) -> ResultItem {
    ResultItem {
        url: None,
        source: p.source,
        provider_id: p.provider_id.clone(),
        row: row(&p.name, p.brand.as_deref(), p.basis, &p.nutrients, lang),
    }
}

pub struct Attribution {
    pub text: &'static str,
    pub url: &'static str,
}

/// Results from every enabled provider, merged into one list.
pub struct RemoteResults {
    pub items: Vec<ResultItem>,
    pub has_more: bool,
    /// One message per provider that could not be searched.
    pub errors: Vec<String>,
    /// A provider refused only because of its rate limit; the page script
    /// retries shortly instead of showing an error.
    pub retry: bool,
}

/// Provider results as list items, appended by the search page script to
/// the list it already shows.
#[derive(Template)]
#[template(path = "remote_results.html")]
pub struct RemoteFragment {
    pub lang: Lang,
    pub items: Vec<ResultItem>,
    pub has_more: bool,
    pub errors: Vec<String>,
    pub retry: bool,
    pub ctx: LogContext,
}

#[derive(Template)]
#[template(path = "search.html")]
pub struct SearchPage {
    pub lang: Lang,
    pub title: String,
    pub nav: &'static str,
    pub q: String,
    pub ctx: LogContext,
    pub items: Vec<ResultItem>,
    pub errors: Vec<String>,
    /// Provider results still to be loaded by the page script.
    pub pending_src: Option<String>,
    pub prev_url: Option<String>,
    pub next_url: Option<String>,
    /// Shown by the script when pending provider results report more pages.
    pub next_if_more_url: Option<String>,
    pub attributions: Vec<Attribution>,
    pub create_url: String,
    pub message: Option<&'static str>,
}

fn search_url(q: &str, ctx: &LogContext, page: usize, extra: &str) -> String {
    let mut url = format!("/search?q={}", url_encode(q));
    let c = ctx.query();
    if !c.is_empty() {
        url.push('&');
        url.push_str(&c);
    }
    if page > 1 {
        url.push_str(&format!("&page={page}"));
    }
    url.push_str(extra);
    url
}

fn create_url(q: &str, ctx: &LogContext) -> String {
    let mut url = ctx.url("/foods/new");
    if !q.is_empty() {
        url.push(if url.contains('?') { '&' } else { '?' });
        url.push_str("name=");
        url.push_str(&url_encode(q));
    }
    url
}

/// Searches every enabled provider. Products already saved locally are
/// left out (the saved food is listed instead), as are duplicates across
/// providers.
async fn remote_results(
    state: &AppState,
    q: &str,
    page: usize,
    lang: Lang,
) -> AppResult<RemoteResults> {
    let barcode = normalize_barcode(q);
    let is_barcode = is_valid_barcode(&barcode);
    let mut results = RemoteResults {
        items: Vec::new(),
        has_more: false,
        errors: Vec::new(),
        retry: false,
    };
    let mut seen: Vec<String> = Vec::new();
    for p in state.providers.all() {
        let result = if is_barcode {
            p.lookup_barcode(&barcode, lang)
                .await
                .map(|found| ProviderPage {
                    foods: found.into_iter().collect(),
                    has_more: false,
                })
        } else {
            p.search(q, page, lang).await
        };
        match result {
            Ok(found) => {
                let codes: Vec<String> = found
                    .foods
                    .iter()
                    .filter_map(|f| f.barcode.clone())
                    .collect();
                let local = foods::existing_barcodes(&state.db, &codes).await?;
                let (saved, new): (Vec<&ProviderFood>, Vec<&ProviderFood>) = found
                    .foods
                    .iter()
                    .partition(|f| f.barcode.as_ref().is_some_and(|b| local.contains(b)));
                let saved: Vec<ProviderFood> = saved.into_iter().cloned().collect();
                foods::refresh_official_names(&state.db, &saved).await?;
                for f in new {
                    if let Some(b) = &f.barcode {
                        if seen.contains(b) {
                            continue;
                        }
                        seen.push(b.clone());
                    }
                    results.items.push(remote_item(f, lang));
                }
                results.has_more |= found.has_more && page < MAX_PAGE;
            }
            Err(ProviderError::RateLimited) => {
                tracing::info!(provider = p.id(), "provider search rate limited");
                results.retry = true;
                results.errors.push(lang.f(
                    "{0} could not be searched: {1}.",
                    &[&p.name(), &ProviderError::RateLimited.message(lang)],
                ));
            }
            Err(e) => {
                tracing::warn!(provider = p.id(), error = ?e, "provider search failed");
                results.errors.push(lang.f(
                    "{0} could not be searched: {1}.",
                    &[&p.name(), &e.message(lang)],
                ));
            }
        }
    }
    Ok(results)
}

pub async fn page(
    State(state): State<AppState>,
    Query(sq): Query<SearchQuery>,
) -> AppResult<Response> {
    let lang = current_lang();
    let q = sq.query();
    let page = sq.page();
    let ctx = sq.ctx();
    let search_remote = !q.is_empty() && !state.providers.is_empty();
    // Without the page script (or when the form is submitted), provider
    // results are fetched before rendering.
    let inline_remote = search_remote && sq.remote.is_some();

    let local = foods::search(&state.db, &q, page, lang).await?;
    let mut items: Vec<ResultItem> = local
        .foods
        .iter()
        .map(|f| local_item(f, &ctx, lang))
        .collect();
    let mut errors = Vec::new();
    let mut has_more = local.has_more;
    if inline_remote {
        let remote = remote_results(&state, &q, page, lang).await?;
        items.extend(remote.items);
        errors = remote.errors;
        has_more |= remote.has_more;
    }
    let pending = search_remote && !inline_remote;
    let extra = if inline_remote { "&remote=1" } else { "" };

    let title = match (ctx.meal, q.is_empty()) {
        (Some(m), _) => m.add_food_title(lang).to_string(),
        (None, true) => lang.t("Foods").into(),
        (None, false) => lang.f("Search: {0}", &[&q]),
    };
    let page_view = SearchPage {
        lang,
        title,
        nav: "foods",
        items,
        errors,
        pending_src: pending.then(|| {
            let mut url = format!("/search/remote?q={}&page={page}", url_encode(&q));
            let c = ctx.query();
            if !c.is_empty() {
                url.push('&');
                url.push_str(&c);
            }
            url
        }),
        prev_url: (page > 1).then(|| search_url(&q, &ctx, page - 1, extra)),
        next_url: has_more.then(|| search_url(&q, &ctx, page + 1, extra)),
        next_if_more_url: (pending && !has_more && page < MAX_PAGE)
            .then(|| search_url(&q, &ctx, page + 1, extra)),
        attributions: if search_remote {
            state
                .providers
                .all()
                .iter()
                .map(|p| Attribution {
                    text: p.attribution(lang),
                    url: p.attribution_url(),
                })
                .collect()
        } else {
            Vec::new()
        },
        create_url: create_url(&q, &ctx),
        message: flash_message(sq.msg.as_deref(), lang),
        q,
        ctx,
    };
    Ok(render(&page_view)?.into_response())
}

/// Provider results as an HTML fragment for the search page script. Without
/// the fetch header it redirects to the full page, so the URL stays usable.
pub async fn remote_fragment(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(sq): Query<SearchQuery>,
) -> AppResult<Response> {
    let q = sq.query();
    let ctx = sq.ctx();
    if !wants_fragment(&headers) {
        return Ok(super::see_other(&search_url(
            &q,
            &ctx,
            sq.page(),
            "&remote=1",
        )));
    }
    let lang = current_lang();
    let remote = if q.chars().count() < 2 || state.providers.is_empty() {
        RemoteResults {
            items: Vec::new(),
            has_more: false,
            errors: Vec::new(),
            retry: false,
        }
    } else {
        remote_results(&state, &q, sq.page(), lang).await?
    };
    let fragment = RemoteFragment {
        lang,
        items: remote.items,
        has_more: remote.has_more,
        errors: remote.errors,
        retry: remote.retry,
        ctx,
    };
    Ok(render(&fragment)?.into_response())
}
