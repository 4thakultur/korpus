//! Open Food Facts provider.
//!
//! Uses the documented read APIs: `/api/v2/product/{barcode}` for exact
//! lookups and the Search-a-licious service (`search.openfoodfacts.org`)
//! for full-text search. Open Food Facts asks
//! clients to send an identifying User-Agent and to stay within roughly
//! 100 product reads and 10 searches per minute; both limits are enforced
//! locally. Data is © Open Food Facts contributors under the ODbL.
//!
//! Language: searches pass `langs=<code>` so Search-a-licious matches the
//! product names, categories and labels written in the interface language.
//! Both endpoints return `product_name_<code>` for every supported language;
//! the one in the requested language is preferred, otherwise the product's
//! main `product_name` is used as is. All of them are kept so a saved food
//! follows later language changes.

use std::time::Duration;

use serde_json::Value;

use super::cache::{RateLimiter, TtlCache};
use super::{BoxFuture, FoodProvider, ProviderError, ProviderFood, ProviderPage};
use crate::config::Config;
use crate::foods::{is_valid_barcode, normalize_barcode};
use crate::i18n::Lang;
use crate::nutrition::{Basis, Nutrient, Nutrients, check_value, kj_to_kcal};

pub const SOURCE: &str = "openfoodfacts";
const PAGE_SIZE: usize = 20;
/// Longest a search waits for the rate limiter before reporting a limit.
const SEARCH_MAX_WAIT: Duration = Duration::from_secs(3);
const FIELDS: &str = "code,product_name,brands,nutriments,nutrition_data_per,serving_size,\
serving_quantity,product_quantity_unit,quantity,last_modified_t";

/// Requested fields, including the product name in every supported language.
fn fields() -> String {
    let mut f = FIELDS.to_string();
    for l in Lang::ALL {
        f.push_str(",product_name_");
        f.push_str(l.code());
    }
    f
}

pub struct OpenFoodFacts {
    client: reqwest::Client,
    base_url: String,
    search_url: String,
    search_cache: TtlCache<ProviderPage>,
    product_cache: TtlCache<Option<ProviderFood>>,
    search_limit: RateLimiter,
    product_limit: RateLimiter,
}

impl OpenFoodFacts {
    pub fn new(config: &Config) -> Result<Self, reqwest::Error> {
        let client = reqwest::Client::builder()
            .user_agent(config.user_agent.clone())
            .timeout(config.provider_timeout)
            .connect_timeout(Duration::from_secs(5))
            .build()?;
        Ok(OpenFoodFacts {
            client,
            base_url: config.off_base_url.clone(),
            search_url: config.off_search_url.clone(),
            search_cache: TtlCache::new(Duration::from_secs(60 * 60), 200),
            product_cache: TtlCache::new(Duration::from_secs(24 * 60 * 60), 500),
            search_limit: RateLimiter::new(10, Duration::from_secs(60)),
            product_limit: RateLimiter::new(90, Duration::from_secs(60)),
        })
    }

    async fn get_json(&self, url: &str, query: &[(&str, &str)]) -> Result<Value, ProviderError> {
        // One retry for transient upstream failures; never more.
        let mut attempt = 0;
        loop {
            attempt += 1;
            let result = self.client.get(url).query(query).send().await;
            let retryable = match &result {
                Ok(r) => matches!(r.status().as_u16(), 502..=504),
                Err(e) => e.is_connect(),
            };
            if retryable && attempt < 2 {
                tokio::time::sleep(Duration::from_millis(300)).await;
                continue;
            }
            let resp = result.map_err(map_reqwest)?;
            let status = resp.status();
            if status.as_u16() == 429 {
                return Err(ProviderError::RateLimited);
            }
            if status.as_u16() == 404 {
                // The product endpoint answers 404 with a JSON body for unknown codes.
                return resp.json::<Value>().await.or(Ok(Value::Null));
            }
            if !status.is_success() {
                return Err(ProviderError::Unavailable(format!("HTTP {status}")));
            }
            return resp.json::<Value>().await.map_err(map_reqwest);
        }
    }

    async fn fetch_product(
        &self,
        code: &str,
        lang: Lang,
    ) -> Result<Option<ProviderFood>, ProviderError> {
        let code = normalize_barcode(code);
        if !is_valid_barcode(&code) {
            return Ok(None);
        }
        let key = format!("{}\u{0}{code}", lang.code());
        if let Some(hit) = self.product_cache.get(&key) {
            return Ok(hit);
        }
        if !self.product_limit.try_acquire() {
            return Err(ProviderError::RateLimited);
        }
        let url = format!("{}/api/v2/product/{}", self.base_url, code);
        let json = self
            .get_json(&url, &[("fields", &fields()), ("lc", lang.code())])
            .await?;
        let product = parse_product_response(&json, lang)?;
        self.product_cache.insert(key, product.clone());
        Ok(product)
    }

    async fn fetch_search(
        &self,
        query: &str,
        page: usize,
        lang: Lang,
    ) -> Result<ProviderPage, ProviderError> {
        let query = query.trim();
        let key = format!(
            "{}\u{0}{}\u{0}{page}",
            lang.code(),
            crate::foods::normalize(query)
        );
        if let Some(hit) = self.search_cache.get(&key) {
            return Ok(hit);
        }
        // Searches follow typing, so bursts are expected: wait briefly for a
        // free slot before giving up.
        if !self.search_limit.acquire(SEARCH_MAX_WAIT).await {
            return Err(ProviderError::RateLimited);
        }
        let url = format!("{}/search", self.search_url);
        let page_s = page.max(1).to_string();
        let size_s = PAGE_SIZE.to_string();
        let json = self
            .get_json(
                &url,
                &[
                    ("q", query),
                    ("langs", lang.code()),
                    ("page", &page_s),
                    ("page_size", &size_s),
                    ("fields", &fields()),
                ],
            )
            .await?;
        let result = parse_search_response(&json, page.max(1), lang)?;
        self.search_cache.insert(key, result.clone());
        Ok(result)
    }
}

fn map_reqwest(e: reqwest::Error) -> ProviderError {
    if e.is_timeout() {
        ProviderError::Timeout
    } else if e.is_decode() {
        ProviderError::InvalidResponse(e.to_string())
    } else {
        ProviderError::Unavailable(e.to_string())
    }
}

impl FoodProvider for OpenFoodFacts {
    fn id(&self) -> &'static str {
        SOURCE
    }
    fn name(&self) -> &'static str {
        "Open Food Facts"
    }
    fn attribution(&self, lang: Lang) -> &'static str {
        lang.t(
            "Product data © Open Food Facts contributors, available under the Open Database License (ODbL).",
        )
    }
    fn attribution_url(&self) -> &'static str {
        "https://world.openfoodfacts.org"
    }
    fn search<'a>(
        &'a self,
        query: &'a str,
        page: usize,
        lang: Lang,
    ) -> BoxFuture<'a, Result<ProviderPage, ProviderError>> {
        Box::pin(self.fetch_search(query, page, lang))
    }
    fn get<'a>(
        &'a self,
        provider_id: &'a str,
        lang: Lang,
    ) -> BoxFuture<'a, Result<Option<ProviderFood>, ProviderError>> {
        Box::pin(self.fetch_product(provider_id, lang))
    }
    fn lookup_barcode<'a>(
        &'a self,
        barcode: &'a str,
        lang: Lang,
    ) -> BoxFuture<'a, Result<Option<ProviderFood>, ProviderError>> {
        Box::pin(self.fetch_product(barcode, lang))
    }
}

pub fn parse_product_response(
    json: &Value,
    lang: Lang,
) -> Result<Option<ProviderFood>, ProviderError> {
    if json.is_null() {
        return Ok(None);
    }
    let status = json.get("status").and_then(Value::as_i64);
    match (status, json.get("product")) {
        (Some(1), Some(p)) => Ok(parse_product(p, lang)),
        (Some(0), _) | (_, None) => Ok(None),
        _ => Err(ProviderError::InvalidResponse(
            "unexpected product response".into(),
        )),
    }
}

/// Parses a Search-a-licious response (`hits`, `page_count`). The legacy
/// `/cgi/search.pl` shape (`products`, `count`) is accepted as well, so a
/// self-hosted mirror exposing the older API still works.
pub fn parse_search_response(
    json: &Value,
    page: usize,
    lang: Lang,
) -> Result<ProviderPage, ProviderError> {
    let products = json
        .get("hits")
        .or_else(|| json.get("products"))
        .and_then(Value::as_array)
        .ok_or_else(|| ProviderError::InvalidResponse("missing search results".into()))?;
    let foods: Vec<ProviderFood> = products
        .iter()
        .filter_map(|p| parse_product(p, lang))
        .collect();
    let has_more = match json.get("page_count").and_then(as_f64) {
        Some(pages) => (page as f64) < pages,
        None => {
            let count = json.get("count").and_then(as_f64).unwrap_or(0.0);
            ((page * PAGE_SIZE) as f64) < count
        }
    };
    Ok(ProviderPage { foods, has_more })
}

/// Numbers may arrive as JSON numbers or numeric strings.
fn as_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().replace(',', ".").parse().ok(),
        _ => None,
    }
    .filter(|f: &f64| f.is_finite())
}

fn str_field(p: &Value, key: &str) -> Option<String> {
    p.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.chars().take(200).collect())
}

/// Liquids are usually sold by volume; Open Food Facts then gives `_100g`
/// values per 100 ml. Infer the basis from the package quantity.
fn infer_basis(p: &Value) -> (Basis, bool) {
    if p.get("nutrition_data_per").and_then(Value::as_str) == Some("100ml") {
        return (Basis::Per100ml, false);
    }
    if let Some(u) = p.get("product_quantity_unit").and_then(Value::as_str) {
        return match u.trim().to_ascii_lowercase().as_str() {
            "ml" | "cl" | "l" => (Basis::Per100ml, true),
            _ => (Basis::Per100g, true),
        };
    }
    let q = p
        .get("quantity")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let is_volume = ["ml", "cl", "dl", " l", "litre", "liter"]
        .iter()
        .any(|u| q.ends_with(u))
        || (q.ends_with('l')
            && q[..q.len() - 1]
                .trim_end()
                .ends_with(|c: char| c.is_ascii_digit()));
    if is_volume {
        (Basis::Per100ml, true)
    } else {
        (Basis::Per100g, true)
    }
}

fn parse_product(p: &Value, lang: Lang) -> Option<ProviderFood> {
    let code = p
        .get("code")
        .and_then(Value::as_str)
        .map(normalize_barcode)?;
    if !is_valid_barcode(&code) {
        return None;
    }
    // Names as written in each language, never translations.
    let names: Vec<(Lang, String)> = Lang::ALL
        .into_iter()
        .filter_map(|l| Some((l, str_field(p, &format!("product_name_{}", l.code()))?)))
        .collect();
    let name = names
        .iter()
        .find(|(l, _)| *l == lang)
        .map(|(_, n)| n.clone())
        .or_else(|| str_field(p, "product_name"))?;
    // `brands` is a comma-separated string (product API) or an array (search API).
    let brand = match p.get("brands") {
        Some(Value::Array(items)) => items.first().and_then(Value::as_str).map(str::to_string),
        _ => str_field(p, "brands").and_then(|b| b.split(',').next().map(str::to_string)),
    }
    .map(|b| b.trim().chars().take(120).collect::<String>())
    .filter(|b| !b.is_empty());
    let (basis, basis_inferred) = infer_basis(p);

    let empty = Value::Null;
    let nutr = p.get("nutriments").unwrap_or(&empty);
    let mut nutrients = Nutrients::default();
    let mut rejected = Vec::new();
    let mut take = |n: Nutrient, raw: Option<f64>| {
        if let Some(v) = raw {
            match check_value(n, basis, v) {
                Ok(v) => nutrients.set(n, Some(v)),
                Err(_) => rejected.push(n),
            }
        }
    };

    let kcal = nutr.get("energy-kcal_100g").and_then(as_f64);
    let kj = nutr.get("energy-kj_100g").and_then(as_f64);
    let energy_from_kj = kcal.is_none() && kj.is_some();
    take(Nutrient::Energy, kcal.or(kj.map(kj_to_kcal)));
    take(
        Nutrient::Protein,
        nutr.get("proteins_100g").and_then(as_f64),
    );
    take(
        Nutrient::Carbs,
        nutr.get("carbohydrates_100g").and_then(as_f64),
    );
    take(Nutrient::Fat, nutr.get("fat_100g").and_then(as_f64));
    take(Nutrient::Fiber, nutr.get("fiber_100g").and_then(as_f64));
    take(Nutrient::Sugars, nutr.get("sugars_100g").and_then(as_f64));
    take(
        Nutrient::SatFat,
        nutr.get("saturated-fat_100g").and_then(as_f64),
    );
    take(Nutrient::Salt, nutr.get("salt_100g").and_then(as_f64));

    let serving_amount = p
        .get("serving_quantity")
        .and_then(as_f64)
        .filter(|v| *v > 0.0 && *v <= 10_000.0);
    let serving_label = str_field(p, "serving_size").map(|s| s.chars().take(60).collect());

    let source_updated_at = p
        .get("last_modified_t")
        .and_then(Value::as_i64)
        .and_then(|t| jiff::Timestamp::from_second(t).ok())
        .map(|ts| ts.strftime("%Y-%m-%d").to_string());

    Some(ProviderFood {
        source: SOURCE,
        provider_id: code.clone(),
        name,
        names,
        brand,
        barcode: Some(code),
        basis,
        basis_inferred,
        nutrients,
        energy_from_kj,
        serving_amount,
        serving_label,
        source_updated_at,
        rejected,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Value {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(path).unwrap();
        serde_json::from_str(&text).unwrap()
    }

    #[test]
    fn parses_product_fixture() {
        let p = parse_product_response(&fixture("off_product.json"), Lang::En)
            .unwrap()
            .unwrap();
        assert_eq!(p.barcode.as_deref(), Some("3017620422003"));
        assert_eq!(p.name, "Nutella");
        assert_eq!(p.brand.as_deref(), Some("Ferrero"));
        assert_eq!(p.basis, Basis::Per100g);
        assert_eq!(p.nutrients.energy_kcal, Some(539.0));
        assert_eq!(p.nutrients.fat_g, Some(30.9));
        assert_eq!(p.nutrients.salt_g, Some(0.107));
        assert_eq!(p.serving_amount, Some(15.0));
        assert_eq!(p.source_updated_at.as_deref(), Some("2025-01-01"));
        assert!(!p.energy_from_kj);
        assert!(p.rejected.is_empty());
    }

    #[test]
    fn unknown_product_is_none() {
        let json: Value =
            serde_json::from_str(r#"{"status":0,"status_verbose":"product not found"}"#).unwrap();
        assert_eq!(parse_product_response(&json, Lang::En).unwrap(), None);
        assert_eq!(
            parse_product_response(&Value::Null, Lang::En).unwrap(),
            None
        );
    }

    #[test]
    fn parses_search_fixture_with_messy_values() {
        let page = parse_search_response(&fixture("off_search.json"), 1, Lang::En).unwrap();
        // The product without a name and the one with a bad code are skipped.
        assert_eq!(page.foods.len(), 2);
        assert!(page.has_more);

        let juice = &page.foods[0];
        assert_eq!(juice.basis, Basis::Per100ml);
        assert!(juice.basis_inferred);
        assert!(juice.energy_from_kj, "only kJ was given");
        assert!((juice.nutrients.energy_kcal.unwrap() - 188.0 / 4.184).abs() < 1e-9);
        assert_eq!(
            juice.nutrients.protein_g,
            Some(0.7),
            "numeric strings are accepted"
        );
        assert_eq!(juice.nutrients.fiber_g, None, "missing stays missing");

        let bad = &page.foods[1];
        assert_eq!(bad.nutrients.fat_g, None);
        assert!(
            bad.rejected.contains(&Nutrient::Fat),
            "negative fat is rejected"
        );
        assert!(
            bad.rejected.contains(&Nutrient::Protein),
            "impossible protein is rejected"
        );
        assert!(bad.import_notes(Lang::En).unwrap().contains("ignored"));
    }

    #[test]
    fn prefers_name_in_requested_language() {
        let page = parse_search_response(&fixture("off_search_fr.json"), 1, Lang::Fr).unwrap();
        let names: Vec<&str> = page.foods.iter().map(|f| f.name.as_str()).collect();
        // A French name is used when present; otherwise the product's own
        // main name is kept untranslated.
        assert_eq!(names, ["Yaourt nature", "Joghurt Natur"]);

        let page = parse_search_response(&fixture("off_search_fr.json"), 1, Lang::Es).unwrap();
        let names: Vec<&str> = page.foods.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["Natural yogurt", "Joghurt Natur"]);
    }

    #[test]
    fn requests_name_in_every_language() {
        assert!(fields().ends_with(",product_name_en,product_name_fr,product_name_es"));
    }

    #[test]
    fn product_name_in_requested_language() {
        let json: Value = serde_json::from_str(
            r#"{"status":1,"product":{"code":"12345678","product_name":"Milk","product_name_es":"Leche"}}"#,
        )
        .unwrap();
        let es = parse_product_response(&json, Lang::Es).unwrap().unwrap();
        assert_eq!(es.name, "Leche");
        assert_eq!(es.names, [(Lang::Es, "Leche".to_string())]);
        let fr = parse_product_response(&json, Lang::Fr).unwrap().unwrap();
        assert_eq!(fr.name, "Milk");
        assert_eq!(fr.names, es.names);
    }

    #[test]
    fn parses_legacy_search_shape() {
        let json: Value = serde_json::from_str(
            r#"{"count": 21, "products": [{"code": "12345678", "product_name": "Old", "brands": "A, B"}]}"#,
        )
        .unwrap();
        let page = parse_search_response(&json, 1, Lang::En).unwrap();
        assert_eq!(page.foods[0].brand.as_deref(), Some("A"));
        assert!(page.has_more);
        assert!(!parse_search_response(&json, 2, Lang::En).unwrap().has_more);
    }

    #[test]
    fn search_without_products_is_invalid() {
        let json: Value = serde_json::from_str(r#"{"error":"x"}"#).unwrap();
        assert!(parse_search_response(&json, 1, Lang::En).is_err());
    }

    #[test]
    fn infers_volume_basis_from_quantity() {
        let q = |s: &str| infer_basis(&serde_json::json!({ "quantity": s })).0;
        assert_eq!(q("1 l"), Basis::Per100ml);
        assert_eq!(q("1l"), Basis::Per100ml);
        assert_eq!(q("33 cl"), Basis::Per100ml);
        assert_eq!(q("500 ml"), Basis::Per100ml);
        assert_eq!(q("400 g"), Basis::Per100g);
        assert_eq!(q("1 kg"), Basis::Per100g);
        assert_eq!(q(""), Basis::Per100g);
    }
}
