//! Foods: model, validation of manual input, local search and persistence.
//!
//! Foods are shared within the installation (Korpus has a single local
//! profile). Imported provider foods are stored as local rows so they stay
//! available when the provider is not.
//!
//! Imported foods also keep the provider's official name in each interface
//! language (`food_names`). Foods loaded for display carry the name for the
//! active language in `name`; names are never machine-translated.

use std::collections::HashMap;

use serde::Deserialize;
use sqlx::{AssertSqlSafe, SqlitePool};

use crate::db::now_utc;
use crate::error::AppResult;
use crate::forms::FieldErrors;
use crate::i18n::Lang;
use crate::nutrition::{Basis, Nutrient, Nutrients, check_value, kj_to_kcal};
use crate::providers::ProviderFood;
use crate::ranking::{self, Candidate, Query};
use crate::units::{clean_text, parse_decimal};

pub const SOURCE_USER: &str = "user";
pub const PAGE_SIZE: usize = 20;
/// Upper bound on candidate rows considered for ranking one local search.
const SEARCH_CANDIDATES: i64 = 200;
pub const MAX_QUERY_CHARS: usize = 100;

#[derive(Clone, Debug, sqlx::FromRow, serde::Serialize, serde::Deserialize)]
pub struct Food {
    pub id: i64,
    pub name: String,
    pub brand: Option<String>,
    pub barcode: Option<String>,
    pub basis: String,
    pub energy_kcal: Option<f64>,
    pub protein_g: Option<f64>,
    pub carbs_g: Option<f64>,
    pub fat_g: Option<f64>,
    pub fiber_g: Option<f64>,
    pub sugars_g: Option<f64>,
    pub sat_fat_g: Option<f64>,
    pub salt_g: Option<f64>,
    pub energy_from_kj: bool,
    pub serving_amount: Option<f64>,
    pub serving_label: Option<String>,
    pub notes: Option<String>,
    pub source: String,
    pub provider_id: Option<String>,
    pub source_updated_at: Option<String>,
    pub fetched_at: Option<String>,
    pub edited_locally: bool,
    pub created_at: String,
    pub updated_at: String,
    /// The provider's official names per language (empty for foods the user
    /// created or renamed).
    #[sqlx(skip)]
    #[serde(default)]
    pub names: Vec<LocalName>,
}

/// A product's official name in one interface language.
#[derive(Clone, Debug, PartialEq, sqlx::FromRow, serde::Serialize, serde::Deserialize)]
pub struct LocalName {
    pub lang: String,
    pub name: String,
}

const FOOD_COLUMNS: &str = "id, name, brand, barcode, basis, energy_kcal, protein_g, carbs_g, \
    fat_g, fiber_g, sugars_g, sat_fat_g, salt_g, energy_from_kj, serving_amount, serving_label, \
    notes, source, provider_id, source_updated_at, fetched_at, edited_locally, created_at, updated_at";

impl Food {
    pub fn basis(&self) -> Basis {
        // The column has a CHECK constraint; default defensively anyway.
        Basis::parse(&self.basis).unwrap_or(Basis::Per100g)
    }

    pub fn nutrients(&self) -> Nutrients {
        Nutrients {
            energy_kcal: self.energy_kcal,
            protein_g: self.protein_g,
            carbs_g: self.carbs_g,
            fat_g: self.fat_g,
            fiber_g: self.fiber_g,
            sugars_g: self.sugars_g,
            sat_fat_g: self.sat_fat_g,
            salt_g: self.salt_g,
        }
    }

    pub fn source_label(&self, lang: Lang) -> &'static str {
        source_label(&self.source, lang)
    }

    pub fn is_user_created(&self) -> bool {
        self.source == SOURCE_USER
    }

    /// The official name in `lang`, if the provider published one.
    pub fn name_in(&self, lang: Lang) -> Option<&str> {
        self.names
            .iter()
            .find(|n| n.lang == lang.code())
            .map(|n| n.name.as_str())
    }
}

/// Official names of the given foods, by food id.
pub async fn load_names(
    db: &SqlitePool,
    ids: impl IntoIterator<Item = i64>,
) -> AppResult<HashMap<i64, Vec<LocalName>>> {
    let ids: Vec<i64> = ids.into_iter().collect();
    let mut map: HashMap<i64, Vec<LocalName>> = HashMap::new();
    // Bounded batches keep the statement under SQLite's variable limit.
    for chunk in ids.chunks(500) {
        let placeholders = vec!["?"; chunk.len()].join(",");
        let sql = format!(
            "SELECT food_id, lang, name FROM food_names WHERE food_id IN ({placeholders})
             ORDER BY lang"
        );
        let mut q = sqlx::query_as::<_, (i64, String, String)>(AssertSqlSafe(sql));
        for id in chunk {
            q = q.bind(id);
        }
        for (id, lang, name) in q.fetch_all(db).await? {
            map.entry(id).or_default().push(LocalName { lang, name });
        }
    }
    Ok(map)
}

/// Attaches official names and, when `lang` is given, puts the name for
/// that language in `name`.
async fn with_names(
    db: &SqlitePool,
    mut foods: Vec<Food>,
    lang: Option<Lang>,
) -> AppResult<Vec<Food>> {
    let mut names = load_names(db, foods.iter().map(|f| f.id)).await?;
    for f in &mut foods {
        f.names = names.remove(&f.id).unwrap_or_default();
        if let Some(local) = lang.and_then(|l| f.name_in(l)) {
            f.name = local.to_string();
        }
    }
    Ok(foods)
}

/// Replaces a food's official names and refreshes its search text.
pub async fn store_names(db: &SqlitePool, id: i64, names: &[(Lang, String)]) -> AppResult<()> {
    let mut tx = db.begin().await?;
    sqlx::query("DELETE FROM food_names WHERE food_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    for (lang, name) in names {
        sqlx::query("INSERT INTO food_names (food_id, lang, name) VALUES (?, ?, ?)")
            .bind(id)
            .bind(lang.code())
            .bind(name)
            .execute(&mut *tx)
            .await?;
    }
    refresh_search_text(&mut tx, id).await?;
    tx.commit().await?;
    Ok(())
}

/// Recomputes the text local search matches from the stored name, the
/// official names, brand and barcode, so a food is found in any language.
async fn refresh_search_text(conn: &mut sqlx::SqliteConnection, id: i64) -> sqlx::Result<()> {
    let (name, brand, barcode): (String, Option<String>, Option<String>) =
        sqlx::query_as("SELECT name, brand, barcode FROM foods WHERE id = ?")
            .bind(id)
            .fetch_one(&mut *conn)
            .await?;
    let official: Vec<String> =
        sqlx::query_scalar("SELECT name FROM food_names WHERE food_id = ? ORDER BY lang")
            .bind(id)
            .fetch_all(&mut *conn)
            .await?;
    let names: Vec<&str> = std::iter::once(name.as_str())
        .chain(official.iter().map(String::as_str))
        .collect();
    sqlx::query("UPDATE foods SET search_text = ? WHERE id = ?")
        .bind(search_text(&names, brand.as_deref(), barcode.as_deref()))
        .bind(id)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// Version of [`normalize`] that stored search text was built with, kept in
/// SQLite's `user_version`. Raise it whenever `normalize` changes.
const SEARCH_TEXT_VERSION: i64 = 1;

/// Rebuilds the search text of saved and table foods when it was built by
/// an older [`normalize`]. Only this derived column is rewritten.
pub async fn reindex_if_needed(db: &SqlitePool) -> sqlx::Result<()> {
    let version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(db)
        .await?;
    if version >= SEARCH_TEXT_VERSION {
        return Ok(());
    }
    let mut tx = db.begin().await?;
    let ids: Vec<i64> = sqlx::query_scalar("SELECT id FROM foods")
        .fetch_all(&mut *tx)
        .await?;
    for id in &ids {
        refresh_search_text(&mut tx, *id).await?;
    }
    type Names = (
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
    );
    let rows: Vec<Names> =
        sqlx::query_as("SELECT dataset, code, name, name_en, name_fr, name_es FROM dataset_foods")
            .fetch_all(&mut *tx)
            .await?;
    for (dataset, code, name, en, fr, es) in &rows {
        let names: Vec<&str> = [Some(name), en.as_ref(), fr.as_ref(), es.as_ref()]
            .into_iter()
            .flatten()
            .map(String::as_str)
            .collect();
        sqlx::query("UPDATE dataset_foods SET search_text = ? WHERE dataset = ? AND code = ?")
            .bind(search_text(&names, None, None))
            .bind(dataset)
            .bind(code)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query(AssertSqlSafe(format!(
        "PRAGMA user_version = {SEARCH_TEXT_VERSION}"
    )))
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    tracing::info!(
        foods = ids.len(),
        table_foods = rows.len(),
        "rebuilt search index"
    );
    Ok(())
}

pub fn source_label(source: &str, lang: Lang) -> &'static str {
    match source {
        SOURCE_USER => lang.t("Created by you"),
        crate::providers::off::SOURCE => "Open Food Facts",
        other => match crate::datasets::Dataset::parse(other) {
            Some(d) => d.name(),
            None => lang.t("Other source"),
        },
    }
}

/// Raw manual food form, kept as strings so it can be re-displayed on error.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct FoodForm {
    pub name: String,
    pub brand: String,
    pub barcode: String,
    pub basis: String,
    pub energy: String,
    pub energy_unit: String,
    pub protein: String,
    pub carbs: String,
    pub fat: String,
    pub fiber: String,
    pub sugars: String,
    pub sat_fat: String,
    pub salt: String,
    pub serving_amount: String,
    pub serving_label: String,
    pub notes: String,
}

impl FoodForm {
    pub fn from_food(f: &Food) -> Self {
        let n = |v: Option<f64>| v.map(format_input).unwrap_or_default();
        FoodForm {
            name: f.name.clone(),
            brand: f.brand.clone().unwrap_or_default(),
            barcode: f.barcode.clone().unwrap_or_default(),
            basis: f.basis.clone(),
            energy: n(f.energy_kcal),
            energy_unit: "kcal".into(),
            protein: n(f.protein_g),
            carbs: n(f.carbs_g),
            fat: n(f.fat_g),
            fiber: n(f.fiber_g),
            sugars: n(f.sugars_g),
            sat_fat: n(f.sat_fat_g),
            salt: n(f.salt_g),
            serving_amount: n(f.serving_amount),
            serving_label: f.serving_label.clone().unwrap_or_default(),
            notes: f.notes.clone().unwrap_or_default(),
        }
    }
}

/// Shows a stored value in an input without float noise (e.g. 59.75597…).
fn format_input(v: f64) -> String {
    let s = format!("{:.3}", v);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// A validated food ready to be stored.
#[derive(Clone, Debug, PartialEq)]
pub struct FoodInput {
    pub name: String,
    pub brand: Option<String>,
    pub barcode: Option<String>,
    pub basis: Basis,
    pub nutrients: Nutrients,
    pub energy_from_kj: bool,
    pub serving_amount: Option<f64>,
    pub serving_label: Option<String>,
    pub notes: Option<String>,
}

pub fn normalize_barcode(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect()
}

pub fn is_valid_barcode(s: &str) -> bool {
    (6..=14).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit())
}

/// Validates a manual food. Energy, protein, carbohydrate and fat are
/// required for manual entries; other nutrients are optional. Values are
/// never adjusted to make macros "add up".
pub fn validate(form: &FoodForm, lang: Lang) -> Result<FoodInput, FieldErrors> {
    let mut errors = FieldErrors::default();

    let name = match clean_text(&form.name, 200) {
        Ok(Some(n)) => Some(n),
        Ok(None) => {
            errors.add("name", lang.t("Enter a name."));
            None
        }
        Err(e) => {
            errors.add("name", e.for_field(lang, lang.t("Name")));
            None
        }
    };
    let brand = clean_text(&form.brand, 120).unwrap_or_else(|e| {
        errors.add("brand", e.for_field(lang, lang.t("Brand")));
        None
    });
    let barcode = {
        let b = normalize_barcode(&form.barcode);
        if b.is_empty() {
            None
        } else if is_valid_barcode(&b) {
            Some(b)
        } else {
            errors.add("barcode", lang.t("A barcode has 6 to 14 digits."));
            None
        }
    };
    let basis = match Basis::parse(&form.basis) {
        Some(b) => b,
        None => {
            errors.add(
                "basis",
                lang.t("Choose whether values are per 100 g or per 100 ml."),
            );
            Basis::Per100g
        }
    };

    let energy_from_kj = match form.energy_unit.as_str() {
        "" | "kcal" => false,
        "kj" => true,
        _ => {
            errors.add("energy", lang.t("Choose kcal or kJ."));
            false
        }
    };

    let mut nutrients = Nutrients::default();
    let fields: [(Nutrient, &'static str, &str, bool); 8] = [
        (Nutrient::Energy, "energy", &form.energy, true),
        (Nutrient::Protein, "protein", &form.protein, true),
        (Nutrient::Carbs, "carbs", &form.carbs, true),
        (Nutrient::Fat, "fat", &form.fat, true),
        (Nutrient::Fiber, "fiber", &form.fiber, false),
        (Nutrient::Sugars, "sugars", &form.sugars, false),
        (Nutrient::SatFat, "sat_fat", &form.sat_fat, false),
        (Nutrient::Salt, "salt", &form.salt, false),
    ];
    for (nutrient, field, raw, required) in fields {
        match parse_decimal(raw) {
            Ok(None) if required => errors.add(
                field,
                lang.f(
                    "Enter {0} (use 0 if none).",
                    &[&nutrient.label(lang).to_lowercase()],
                ),
            ),
            Ok(None) => {}
            Ok(Some(mut v)) => {
                if nutrient == Nutrient::Energy && energy_from_kj {
                    if v < 0.0 {
                        errors.add(field, lang.t("Energy cannot be negative."));
                        continue;
                    }
                    v = kj_to_kcal(v);
                }
                match check_value(nutrient, basis, v) {
                    Ok(v) => nutrients.set(nutrient, Some(v)),
                    Err(e) => errors.add(
                        field,
                        lang.f(
                            "{0} {1} ({2} {3}).",
                            &[
                                &nutrient.label(lang),
                                &e.message(lang),
                                &nutrient.unit(),
                                &basis.label(lang),
                            ],
                        ),
                    ),
                }
            }
            Err(e) => errors.add(field, e.for_field(lang, nutrient.label(lang))),
        }
    }

    let serving_amount = match parse_decimal(&form.serving_amount) {
        Ok(None) => None,
        Ok(Some(v)) if v > 0.0 && v <= 10_000.0 => Some(v),
        Ok(Some(_)) => {
            errors.add(
                "serving_amount",
                lang.t("Serving size must be between 0 and 10000."),
            );
            None
        }
        Err(e) => {
            errors.add("serving_amount", e.for_field(lang, lang.t("Serving size")));
            None
        }
    };
    let serving_label = clean_text(&form.serving_label, 60).unwrap_or_else(|e| {
        errors.add("serving_label", e.for_field(lang, lang.t("Serving name")));
        None
    });
    let notes = clean_text(&form.notes, 2000).unwrap_or_else(|e| {
        errors.add("notes", e.for_field(lang, lang.t("Notes")));
        None
    });

    match name {
        Some(name) if errors.is_empty() => Ok(FoodInput {
            name,
            brand,
            barcode,
            basis,
            nutrients,
            energy_from_kj,
            serving_amount,
            serving_label,
            notes,
        }),
        _ => Err(errors),
    }
}

/// Lower-cases and folds common accents so "Müsli" matches "musli" and
/// "muesli" matches "Muesli", and ligatures so "Œufs" matches "oeufs".
/// Punctuation becomes whitespace. Changing it requires re-indexing stored
/// search text (see [`reindex`]).
pub fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars().flat_map(char::to_lowercase) {
        let folded: &str = match c {
            'à' | 'á' | 'â' | 'ã' | 'å' | 'ā' => "a",
            'ä' => "a",
            'æ' => "ae",
            'ç' | 'č' | 'ć' => "c",
            'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ę' => "e",
            'ì' | 'í' | 'î' | 'ï' => "i",
            'ñ' | 'ń' => "n",
            'ò' | 'ó' | 'ô' | 'õ' | 'ø' | 'ō' => "o",
            'ö' => "o",
            'œ' => "oe",
            // "Éco+" is spoken and typed "eco plus".
            '+' => " plus ",
            'ù' | 'ú' | 'û' | 'ū' => "u",
            'ü' => "u",
            'ý' | 'ÿ' => "y",
            'ß' => "ss",
            'ł' => "l",
            'š' | 'ś' => "s",
            'ž' | 'ź' | 'ż' => "z",
            c if c.is_alphanumeric() => {
                out.push(c);
                continue;
            }
            _ => " ",
        };
        out.push_str(folded);
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Normalized text matched by local search: every name, the brand and the
/// barcode.
pub fn search_text(names: &[&str], brand: Option<&str>, barcode: Option<&str>) -> String {
    let mut seen: Vec<String> = Vec::new();
    for n in names {
        let n = normalize(n);
        if !n.is_empty() && !seen.contains(&n) {
            seen.push(n);
        }
    }
    let mut s = seen.join(" ");
    if let Some(b) = brand {
        s.push(' ');
        s.push_str(&normalize(b));
    }
    if let Some(b) = barcode {
        s.push(' ');
        s.push_str(b);
    }
    s
}

pub fn escape_like(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// How many diary entries use each of the given foods.
async fn usage_counts(db: &SqlitePool, ids: &[i64]) -> AppResult<HashMap<i64, u64>> {
    let mut map = HashMap::new();
    for chunk in ids.chunks(500) {
        let placeholders = vec!["?"; chunk.len()].join(",");
        let sql = format!(
            "SELECT food_id, COUNT(*) FROM diary_entries WHERE food_id IN ({placeholders})
             GROUP BY food_id"
        );
        let mut q = sqlx::query_as::<_, (i64, i64)>(AssertSqlSafe(sql));
        for id in chunk {
            q = q.bind(id);
        }
        for (id, n) in q.fetch_all(db).await? {
            map.insert(id, n.max(0) as u64);
        }
    }
    Ok(map)
}

pub struct SearchPage {
    pub foods: Vec<Food>,
    pub has_more: bool,
}

/// Foods logged in the diary since the history was last cleared, most
/// recently logged first, with names in `lang`. Foods that were only opened
/// or saved are not part of it.
pub async fn history(db: &SqlitePool, page: usize, lang: Lang) -> AppResult<SearchPage> {
    let offset = page.saturating_sub(1) * PAGE_SIZE;
    let mut foods = sqlx::query_as::<_, Food>(AssertSqlSafe(format!(
        "SELECT {FOOD_COLUMNS} FROM foods
         JOIN (SELECT food_id, MAX(created_at) AS last_logged, MAX(id) AS last_entry
               FROM diary_entries
               WHERE food_id IS NOT NULL
                 AND created_at > COALESCE((SELECT history_cleared_at FROM profile WHERE id = 1), '')
               GROUP BY food_id) AS logged ON logged.food_id = foods.id
         ORDER BY logged.last_logged DESC, logged.last_entry DESC LIMIT ? OFFSET ?"
    )))
    .bind((PAGE_SIZE + 1) as i64)
    .bind(offset as i64)
    .fetch_all(db)
    .await?;
    let has_more = foods.len() > PAGE_SIZE;
    foods.truncate(PAGE_SIZE);
    let foods = with_names(db, foods, Some(lang)).await?;
    Ok(SearchPage { foods, has_more })
}

/// Empties the history of logged foods. Diary entries and saved foods are
/// kept; foods logged afterwards appear again.
pub async fn clear_history(db: &SqlitePool) -> AppResult<()> {
    sqlx::query(
        "UPDATE profile SET history_cleared_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = 1",
    )
    .execute(db)
    .await?;
    Ok(())
}

/// Searches local foods, with names in `lang`. An empty query lists the
/// history of logged foods.
pub async fn search(
    db: &SqlitePool,
    query: &str,
    page: usize,
    lang: Lang,
) -> AppResult<SearchPage> {
    let q = normalize(query);
    let offset = page.saturating_sub(1) * PAGE_SIZE;
    if q.is_empty() {
        return history(db, page, lang).await;
    }

    let query = Query::new(&q);
    let terms = query.retrieval_terms();
    // Saved foods are few, so any part of a word finds them ("apple" finds
    // "Pineapple"); ranking puts whole-word matches first.
    let sql = format!(
        "SELECT {FOOD_COLUMNS} FROM foods WHERE {} ORDER BY updated_at DESC LIMIT ?",
        vec!["search_text LIKE ? ESCAPE '\\'"; terms.len()].join(" AND ")
    );
    // Only constant SQL and `?` placeholders are concatenated; values are bound.
    let mut stmt = sqlx::query_as::<_, Food>(AssertSqlSafe(sql));
    for t in &terms {
        stmt = stmt.bind(format!("%{}%", escape_like(t)));
    }
    let foods = stmt.bind(SEARCH_CANDIDATES).fetch_all(db).await?;
    let foods = with_names(db, foods, Some(lang)).await?;

    let ids: Vec<i64> = foods.iter().map(|f| f.id).collect();
    let uses = usage_counts(db, &ids).await?;
    let foods = ranking::sort(&query, foods, |f| Candidate {
        name: &f.name,
        other_names: f
            .names
            .iter()
            .map(|n| n.name.as_str())
            .filter(|n| *n != f.name)
            .collect(),
        brands: f.brand.as_slice(),
        popularity: uses.get(&f.id).copied().unwrap_or(0),
        has_energy: f.energy_kcal.is_some(),
    });
    // An exact barcode match comes first whatever its name.
    let (mut foods, rest): (Vec<Food>, Vec<Food>) = foods
        .into_iter()
        .partition(|f| f.barcode.as_deref() == Some(q.as_str()));
    foods.extend(rest);
    let has_more = foods.len() > offset + PAGE_SIZE;
    let foods = foods.into_iter().skip(offset).take(PAGE_SIZE).collect();
    Ok(SearchPage { foods, has_more })
}

/// Loads a food with its name in `lang`.
pub async fn get(db: &SqlitePool, id: i64, lang: Lang) -> AppResult<Option<Food>> {
    let food = sqlx::query_as::<_, Food>(AssertSqlSafe(format!(
        "SELECT {FOOD_COLUMNS} FROM foods WHERE id = ?"
    )))
    .bind(id)
    .fetch_optional(db)
    .await?;
    Ok(with_names(db, food.into_iter().collect(), Some(lang))
        .await?
        .pop())
}

pub async fn find_by_barcode(db: &SqlitePool, barcode: &str) -> AppResult<Option<Food>> {
    Ok(sqlx::query_as::<_, Food>(AssertSqlSafe(format!(
        "SELECT {FOOD_COLUMNS} FROM foods WHERE barcode = ?"
    )))
    .bind(barcode)
    .fetch_optional(db)
    .await?)
}

/// All foods with their stored names and official names, for export.
pub async fn all_stored(db: &SqlitePool) -> AppResult<Vec<Food>> {
    let foods = sqlx::query_as::<_, Food>(AssertSqlSafe(format!(
        "SELECT {FOOD_COLUMNS} FROM foods ORDER BY id"
    )))
    .fetch_all(db)
    .await?;
    with_names(db, foods, None).await
}

/// Barcodes of local foods among `barcodes`, used to hide provider results
/// that duplicate a food already in the local database.
pub async fn existing_barcodes(db: &SqlitePool, barcodes: &[String]) -> AppResult<Vec<String>> {
    if barcodes.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = vec!["?"; barcodes.len()].join(",");
    let sql = format!("SELECT barcode FROM foods WHERE barcode IN ({placeholders})");
    let mut q = sqlx::query_scalar::<_, String>(AssertSqlSafe(sql));
    for b in barcodes {
        q = q.bind(b);
    }
    Ok(q.fetch_all(db).await?)
}

#[derive(Debug)]
pub enum SaveError {
    DuplicateBarcode(i64),
    Db(sqlx::Error),
}

impl From<sqlx::Error> for SaveError {
    fn from(e: sqlx::Error) -> Self {
        SaveError::Db(e)
    }
}

async fn barcode_conflict(
    db: &SqlitePool,
    barcode: Option<&str>,
    except_id: Option<i64>,
) -> Result<Option<i64>, sqlx::Error> {
    let Some(b) = barcode else { return Ok(None) };
    sqlx::query_scalar::<_, i64>("SELECT id FROM foods WHERE barcode = ? AND id IS NOT ?")
        .bind(b)
        .bind(except_id)
        .fetch_optional(db)
        .await
}

pub async fn create(db: &SqlitePool, f: &FoodInput) -> Result<i64, SaveError> {
    if let Some(id) = barcode_conflict(db, f.barcode.as_deref(), None).await? {
        return Err(SaveError::DuplicateBarcode(id));
    }
    let n = &f.nutrients;
    let id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO foods (name, brand, barcode, basis, energy_kcal, protein_g, carbs_g, fat_g,
             fiber_g, sugars_g, sat_fat_g, salt_g, energy_from_kj, serving_amount, serving_label,
             notes, source, search_text)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(&f.name)
    .bind(&f.brand)
    .bind(&f.barcode)
    .bind(f.basis.as_str())
    .bind(n.energy_kcal)
    .bind(n.protein_g)
    .bind(n.carbs_g)
    .bind(n.fat_g)
    .bind(n.fiber_g)
    .bind(n.sugars_g)
    .bind(n.sat_fat_g)
    .bind(n.salt_g)
    .bind(f.energy_from_kj)
    .bind(f.serving_amount)
    .bind(&f.serving_label)
    .bind(&f.notes)
    .bind(SOURCE_USER)
    .bind(search_text(
        &[&f.name],
        f.brand.as_deref(),
        f.barcode.as_deref(),
    ))
    .fetch_one(db)
    .await?;
    Ok(id)
}

/// Updates a food. Existing diary entries keep their own nutrition snapshot
/// and are not affected.
///
/// `shown_name` is the name the edit form displayed (the official name in
/// the interface language, if any). If the name comes back unchanged, the
/// food keeps its stored and official names. A new name replaces them all:
/// the user's own name is then used in every language.
pub async fn update(
    db: &SqlitePool,
    id: i64,
    f: &FoodInput,
    shown_name: &str,
) -> Result<bool, SaveError> {
    if let Some(other) = barcode_conflict(db, f.barcode.as_deref(), Some(id)).await? {
        return Err(SaveError::DuplicateBarcode(other));
    }
    let renamed = f.name != shown_name;
    let mut tx = db.begin().await?;
    let n = &f.nutrients;
    let res = sqlx::query(
        "UPDATE foods SET name = CASE WHEN ? THEN ? ELSE name END, brand = ?, barcode = ?,
             basis = ?, energy_kcal = ?, protein_g = ?, carbs_g = ?, fat_g = ?, fiber_g = ?,
             sugars_g = ?, sat_fat_g = ?, salt_g = ?, energy_from_kj = ?, serving_amount = ?,
             serving_label = ?, notes = ?, edited_locally = (source <> 'user'),
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
         WHERE id = ?",
    )
    .bind(renamed)
    .bind(&f.name)
    .bind(&f.brand)
    .bind(&f.barcode)
    .bind(f.basis.as_str())
    .bind(n.energy_kcal)
    .bind(n.protein_g)
    .bind(n.carbs_g)
    .bind(n.fat_g)
    .bind(n.fiber_g)
    .bind(n.sugars_g)
    .bind(n.sat_fat_g)
    .bind(n.salt_g)
    .bind(f.energy_from_kj)
    .bind(f.serving_amount)
    .bind(&f.serving_label)
    .bind(&f.notes)
    .bind(id)
    .execute(&mut *tx)
    .await?;
    if res.rows_affected() == 0 {
        return Ok(false);
    }
    if renamed {
        sqlx::query("DELETE FROM food_names WHERE food_id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    refresh_search_text(&mut tx, id).await?;
    tx.commit().await?;
    Ok(true)
}

pub async fn delete(db: &SqlitePool, id: i64) -> AppResult<bool> {
    // Diary entries keep their snapshot; the foreign key sets food_id to NULL.
    let res = sqlx::query("DELETE FROM foods WHERE id = ?")
        .bind(id)
        .execute(db)
        .await?;
    Ok(res.rows_affected() > 0)
}

pub async fn count_diary_references(db: &SqlitePool, id: i64) -> AppResult<i64> {
    Ok(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM diary_entries WHERE food_id = ?")
            .bind(id)
            .fetch_one(db)
            .await?,
    )
}

/// Stores a provider food locally and returns its id.
///
/// * A food already imported from the same provider record is refreshed,
///   unless the user edited it locally (their edits win).
/// * If a different local food already has the barcode, that food is used.
pub async fn upsert_from_provider(db: &SqlitePool, p: &ProviderFood, lang: Lang) -> AppResult<i64> {
    let existing: Option<(i64, bool)> =
        sqlx::query_as("SELECT id, edited_locally FROM foods WHERE source = ? AND provider_id = ?")
            .bind(p.source)
            .bind(&p.provider_id)
            .fetch_optional(db)
            .await?;

    let notes = p.import_notes(lang);
    let n = &p.nutrients;
    let fetched = now_utc();
    let text = search_text(&[&p.name], p.brand.as_deref(), p.barcode.as_deref());

    if let Some((id, edited)) = existing {
        if !edited {
            sqlx::query(
                "UPDATE foods SET name = ?, brand = ?, basis = ?, energy_kcal = ?, protein_g = ?,
                     carbs_g = ?, fat_g = ?, fiber_g = ?, sugars_g = ?, sat_fat_g = ?, salt_g = ?,
                     energy_from_kj = ?, serving_amount = ?, serving_label = ?, notes = ?,
                     source_updated_at = ?, fetched_at = ?, search_text = ?,
                     updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
                 WHERE id = ?",
            )
            .bind(&p.name)
            .bind(&p.brand)
            .bind(p.basis.as_str())
            .bind(n.energy_kcal)
            .bind(n.protein_g)
            .bind(n.carbs_g)
            .bind(n.fat_g)
            .bind(n.fiber_g)
            .bind(n.sugars_g)
            .bind(n.sat_fat_g)
            .bind(n.salt_g)
            .bind(p.energy_from_kj)
            .bind(p.serving_amount)
            .bind(&p.serving_label)
            .bind(&notes)
            .bind(&p.source_updated_at)
            .bind(&fetched)
            .bind(&text)
            .bind(id)
            .execute(db)
            .await?;
            store_names(db, id, &p.names).await?;
        }
        return Ok(id);
    }

    if let Some(b) = &p.barcode
        && let Some(food) = find_by_barcode(db, b).await?
    {
        return Ok(food.id);
    }

    let id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO foods (name, brand, barcode, basis, energy_kcal, protein_g, carbs_g, fat_g,
             fiber_g, sugars_g, sat_fat_g, salt_g, energy_from_kj, serving_amount, serving_label,
             notes, source, provider_id, source_updated_at, fetched_at, search_text)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(&p.name)
    .bind(&p.brand)
    .bind(&p.barcode)
    .bind(p.basis.as_str())
    .bind(n.energy_kcal)
    .bind(n.protein_g)
    .bind(n.carbs_g)
    .bind(n.fat_g)
    .bind(n.fiber_g)
    .bind(n.sugars_g)
    .bind(n.sat_fat_g)
    .bind(n.salt_g)
    .bind(p.energy_from_kj)
    .bind(p.serving_amount)
    .bind(&p.serving_label)
    .bind(&notes)
    .bind(p.source)
    .bind(&p.provider_id)
    .bind(&p.source_updated_at)
    .bind(&fetched)
    .bind(&text)
    .fetch_one(db)
    .await?;
    store_names(db, id, &p.names).await?;
    Ok(id)
}

/// Stores the official names of provider products that are already saved
/// locally (and not edited), so foods imported before names were kept in
/// every language catch up when they appear in a search.
pub async fn refresh_official_names(db: &SqlitePool, products: &[ProviderFood]) -> AppResult<()> {
    for p in products.iter().filter(|p| !p.names.is_empty()) {
        let id: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM foods f WHERE source = ? AND provider_id = ? AND edited_locally = 0
                 AND NOT EXISTS (SELECT 1 FROM food_names n WHERE n.food_id = f.id)",
        )
        .bind(p.source)
        .bind(&p.provider_id)
        .fetch_optional(db)
        .await?;
        if let Some(id) = id {
            store_names(db, id, &p.names).await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form() -> FoodForm {
        FoodForm {
            name: "  Rolled oats ".into(),
            basis: "g".into(),
            energy: "372".into(),
            protein: "13,5".into(),
            carbs: "58.7".into(),
            fat: "7".into(),
            ..Default::default()
        }
    }

    #[test]
    fn valid_manual_food() {
        let f = validate(&form(), Lang::En).unwrap();
        assert_eq!(f.name, "Rolled oats");
        assert_eq!(f.basis, Basis::Per100g);
        assert_eq!(f.nutrients.protein_g, Some(13.5));
        assert_eq!(f.nutrients.fiber_g, None);
        assert!(!f.energy_from_kj);
    }

    #[test]
    fn requires_name_basis_and_core_nutrients() {
        let e = validate(&FoodForm::default(), Lang::En).unwrap_err();
        for field in ["name", "basis", "energy", "protein", "carbs", "fat"] {
            assert!(e.has(field), "missing error for {field}");
        }
        assert!(!e.has("fiber"));
    }

    #[test]
    fn rejects_negative_non_finite_and_implausible() {
        let mut f = form();
        f.fat = "-1".into();
        f.protein = "NaN".into();
        f.carbs = "150".into();
        f.salt = "abc".into();
        let e = validate(&f, Lang::En).unwrap_err();
        assert!(e.has("fat"));
        assert!(e.has("protein"));
        assert!(e.has("carbs"));
        assert!(e.has("salt"));
    }

    #[test]
    fn converts_kj_and_records_it() {
        let mut f = form();
        f.energy = "1556".into();
        f.energy_unit = "kj".into();
        let v = validate(&f, Lang::En).unwrap();
        assert!(v.energy_from_kj);
        assert!((v.nutrients.energy_kcal.unwrap() - 1556.0 / 4.184).abs() < 1e-9);
    }

    #[test]
    fn does_not_force_macros_to_add_up() {
        let mut f = form();
        f.protein = "0".into();
        f.carbs = "0".into();
        f.fat = "0".into();
        f.energy = "500".into();
        assert!(validate(&f, Lang::En).is_ok());
    }

    #[test]
    fn validates_barcode() {
        let mut f = form();
        f.barcode = "4006 3810-0004".into();
        assert_eq!(
            validate(&f, Lang::En).unwrap().barcode.as_deref(),
            Some("400638100004")
        );
        f.barcode = "12ab".into();
        assert!(validate(&f, Lang::En).unwrap_err().has("barcode"));
    }

    #[test]
    fn validation_messages_follow_language() {
        let e = validate(&FoodForm::default(), Lang::Fr).unwrap_err();
        assert_eq!(e.get("name"), Some("Saisissez un nom."));
        let mut f = form();
        f.fat = "-1".into();
        let e = validate(&f, Lang::Es).unwrap_err();
        assert_eq!(
            e.get("fat"),
            Some("Grasas: no puede ser negativo (g por 100 g).")
        );
    }

    #[test]
    fn normalizes_for_search() {
        assert_eq!(normalize("  Müsli, Crunchy!  "), "musli crunchy");
        assert_eq!(normalize("Crème Fraîche"), "creme fraiche");
        assert_eq!(normalize("Weißbrot"), "weissbrot");
        assert_eq!(normalize("50%"), "50");
        assert_eq!(normalize("Œufs, cœur"), "oeufs coeur");
        assert_eq!(normalize("Éco+"), "eco plus");
    }

    #[test]
    fn escapes_like_wildcards() {
        assert_eq!(escape_like("a%b_c\\"), "a\\%b\\_c\\\\");
    }

    #[test]
    fn input_format_drops_float_noise() {
        assert_eq!(format_input(59.755_975_143_403_44), "59.756");
        assert_eq!(format_input(12.0), "12");
        assert_eq!(format_input(0.5), "0.5");
    }

    #[tokio::test]
    async fn reindex_rebuilds_stale_search_text() {
        let db = crate::db::connect_memory().await.unwrap();
        let mut form = form();
        form.name = "Œufs plein air".into();
        let input = validate(&form, Lang::Fr).unwrap();
        let id = create(&db, &input).await.unwrap();
        // As written by the previous normalization, which folded "œ" to "o".
        sqlx::query("UPDATE foods SET search_text = 'oufs plein air' WHERE id = ?")
            .bind(id)
            .execute(&db)
            .await
            .unwrap();
        sqlx::query("PRAGMA user_version = 0")
            .execute(&db)
            .await
            .unwrap();
        assert!(
            search(&db, "oeufs", 1, Lang::Fr)
                .await
                .unwrap()
                .foods
                .is_empty()
        );

        reindex_if_needed(&db).await.unwrap();
        let found = search(&db, "oeufs", 1, Lang::Fr).await.unwrap();
        assert_eq!(found.foods.len(), 1);
        let version: i64 = sqlx::query_scalar("PRAGMA user_version")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(version, SEARCH_TEXT_VERSION);
    }
}
