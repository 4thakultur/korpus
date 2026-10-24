//! National food composition tables, imported from files the administrator
//! downloads from each publisher and searched locally.
//!
//! These tables complement Open Food Facts: they describe generic foods
//! ("apple, raw", "rice, cooked") with laboratory or official values rather
//! than packaged products. Once imported they need no network access, and
//! search terms never leave the server.
//!
//! The profile's country picks one table: the first imported table in
//! [`preference`] for that country. Picking a result copies it into
//! `foods` like a provider product, so diary entries keep working when a
//! table is replaced or removed.
//!
//! One table, CIQUAL, is also shipped with Korpus and installed on first
//! start (see [`bundled`]).
//!
//! Values are stored as published. The only conversions are kJ to kcal and
//! sodium to salt, both recorded per food and explained in its notes.

mod afcd;
mod archive;
mod bundled;
mod ciqual;
mod cnf;
mod cofid;
mod csv;
mod usda;
mod value;

use sqlx::{AssertSqlSafe, SqlitePool};

use crate::countries;
use crate::db::now_utc;
use crate::error::AppResult;
use crate::i18n::Lang;
use crate::nutrition::{Basis, Nutrient, Nutrients};
use crate::providers::ProviderFood;
use crate::ranking::{self, Candidate, Query};

pub use archive::UploadFile;
pub use bundled::{BUNDLED, Bundled, install_new as install_bundled};
pub use ciqual::extract as ciqual_extract;
pub use value::FoodBuilder;

/// Largest upload accepted for one dataset. The biggest supported download
/// is CIQUAL 2025, whose unzipped `compo_*.xml` is about 69 MB.
pub const MAX_UPLOAD_BYTES: usize = 96 * 1024 * 1024;
const PAGE_SIZE: usize = crate::foods::PAGE_SIZE;
/// Upper bound on candidate rows considered for ranking one search.
const SEARCH_CANDIDATES: i64 = 300;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dataset {
    Usda,
    Ciqual,
    Cofid,
    Afcd,
    Cnf,
}

impl Dataset {
    pub const ALL: [Dataset; 5] = [
        Dataset::Ciqual,
        Dataset::Cofid,
        Dataset::Usda,
        Dataset::Cnf,
        Dataset::Afcd,
    ];

    /// Stable identifier, stored in `datasets.id` and `foods.source`.
    pub fn id(self) -> &'static str {
        match self {
            Dataset::Usda => "usda",
            Dataset::Ciqual => "ciqual",
            Dataset::Cofid => "cofid",
            Dataset::Afcd => "afcd",
            Dataset::Cnf => "cnf",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Dataset::ALL.into_iter().find(|d| d.id() == s)
    }

    pub fn name(self) -> &'static str {
        match self {
            Dataset::Usda => "USDA FoodData Central",
            Dataset::Ciqual => "ANSES-CIQUAL",
            Dataset::Cofid => "CoFID (McCance and Widdowson's)",
            Dataset::Afcd => "Australian Food Composition Database",
            Dataset::Cnf => "Canadian Nutrient File",
        }
    }

    /// Country the table describes.
    pub fn country(self) -> &'static str {
        match self {
            Dataset::Usda => "US",
            Dataset::Ciqual => "FR",
            Dataset::Cofid => "GB",
            Dataset::Afcd => "AU",
            Dataset::Cnf => "CA",
        }
    }

    pub fn country_name(self, lang: Lang) -> &'static str {
        countries::name(self.country(), lang).unwrap_or("")
    }

    pub fn attribution(self, lang: Lang) -> &'static str {
        match self {
            Dataset::Usda => lang.t(
                "USDA FoodData Central, U.S. Department of Agriculture, Agricultural Research Service. Public domain (CC0 1.0).",
            ),
            Dataset::Ciqual => lang.t(
                "ANSES-CIQUAL French food composition table. Licence Ouverte / Open Licence (Etalab).",
            ),
            Dataset::Cofid => lang.t(
                "McCance and Widdowson's Composition of Foods Integrated Dataset, Public Health England. Open Government Licence v3.0.",
            ),
            Dataset::Afcd => lang.t(
                "Australian Food Composition Database, © Food Standards Australia New Zealand, used under its data user licence. FSANZ does not endorse Korpus.",
            ),
            Dataset::Cnf => lang.t(
                "Canadian Nutrient File, Health Canada. Open Government Licence – Canada.",
            ),
        }
    }

    /// Official page where the administrator downloads the file(s).
    pub fn download_url(self) -> &'static str {
        match self {
            Dataset::Usda => "https://fdc.nal.usda.gov/download-datasets",
            Dataset::Ciqual => "https://doi.org/10.57745/RDMHWY",
            Dataset::Cofid => {
                "https://www.gov.uk/government/publications/composition-of-foods-integrated-dataset-cofid"
            }
            Dataset::Afcd => {
                "https://www.foodstandards.gov.au/science-data/food-nutrient-databases/afcd/australian-food-composition-database-download-excel-files"
            }
            Dataset::Cnf => {
                "https://www.canada.ca/en/health-canada/services/food-nutrition/healthy-eating/nutrient-data/canadian-nutrient-file-2015-download-files.html"
            }
        }
    }

    /// Which file to download from [`Dataset::download_url`].
    pub fn file_hint(self, lang: Lang) -> &'static str {
        match self {
            Dataset::Usda => lang.t(
                "Download the CSV zip files for Foundation Foods and SR Legacy (and optionally FNDDS survey foods). You can upload several at once. Branded foods are not supported.",
            ),
            Dataset::Ciqual => lang.t(
                "Download the XML files alim_….xml and compo_….xml and upload both at once (or the XML zip file of the 2020 release).",
            ),
            Dataset::Cofid => lang.t(
                "Download the Excel file of the Composition of Foods Integrated Dataset.",
            ),
            Dataset::Afcd => lang.t("Download the Excel file “Nutrient profiles”."),
            Dataset::Cnf => lang.t("Download the CSV zip file of the Canadian Nutrient File."),
        }
    }

    /// File types offered by the upload field.
    pub fn accept(self) -> &'static str {
        match self {
            Dataset::Cofid | Dataset::Afcd => {
                ".xlsx,application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
            }
            Dataset::Ciqual => ".xml,.zip,text/xml,application/xml,application/zip",
            Dataset::Usda | Dataset::Cnf => ".zip,application/zip",
        }
    }

    /// The table is published as several files that are uploaded together.
    pub fn multiple_files(self) -> bool {
        matches!(self, Dataset::Usda | Dataset::Ciqual)
    }

    /// Languages its food names are published in, main language first.
    pub fn languages(self) -> &'static [Lang] {
        match self {
            Dataset::Ciqual => &[Lang::Fr, Lang::En],
            Dataset::Cnf => &[Lang::En, Lang::Fr],
            Dataset::Usda | Dataset::Cofid | Dataset::Afcd => &[Lang::En],
        }
    }

    /// Reads the publisher's file(s) into foods. CPU-bound; call it from a
    /// blocking task.
    pub fn parse_files(self, files: &[UploadFile]) -> Result<Vec<DatasetFood>, ImportError> {
        if files.is_empty() {
            return Err(ImportError::NoFile);
        }
        let foods = match self {
            Dataset::Usda => usda::parse(files)?,
            Dataset::Ciqual => ciqual::parse(files)?,
            Dataset::Cofid => cofid::parse(files)?,
            Dataset::Afcd => afcd::parse(files)?,
            Dataset::Cnf => cnf::parse(files)?,
        };
        if foods.is_empty() {
            return Err(ImportError::NoFoods);
        }
        Ok(foods)
    }
}

/// Tables in order of preference for a country: its own table, then the
/// closest one (European tables share EU labelling rules), then the rest.
pub fn preference(country: Option<&str>) -> [Dataset; 5] {
    use Dataset::*;
    match country {
        Some("FR" | "BE" | "LU" | "MC") => [Ciqual, Cofid, Usda, Cnf, Afcd],
        Some("GB" | "IE") => [Cofid, Ciqual, Usda, Afcd, Cnf],
        Some("US" | "PR") => [Usda, Cnf, Cofid, Afcd, Ciqual],
        Some("CA") => [Cnf, Usda, Ciqual, Cofid, Afcd],
        Some("AU" | "NZ") => [Afcd, Cofid, Usda, Cnf, Ciqual],
        Some(c) if countries::is_european(c) => [Ciqual, Cofid, Usda, Afcd, Cnf],
        _ => [Usda, Cnf, Cofid, Ciqual, Afcd],
    }
}

/// The table search uses: the first imported one in the country's order.
pub fn choose(country: Option<&str>, installed: &[Installed]) -> Option<Dataset> {
    preference(country)
        .into_iter()
        .find(|d| installed.iter().any(|i| i.dataset == *d))
}

/// A food read from a publisher's file, before it is stored.
#[derive(Clone, Debug, PartialEq)]
pub struct DatasetFood {
    pub code: String,
    /// Names by language, the table's main language first.
    pub names: Vec<(Lang, String)>,
    pub basis: Basis,
    pub nutrients: Nutrients,
    pub energy_from_kj: bool,
    pub salt_from_sodium: bool,
    pub carbs_include_fiber: bool,
    pub trace: Vec<Nutrient>,
    pub rejected: Vec<Nutrient>,
}

impl DatasetFood {
    pub fn name(&self) -> &str {
        self.names.first().map(|(_, n)| n.as_str()).unwrap_or("")
    }

    fn name_in(&self, lang: Lang) -> Option<&str> {
        self.names
            .iter()
            .find(|(l, _)| *l == lang)
            .map(|(_, n)| n.as_str())
    }
}

#[derive(Debug, PartialEq, thiserror::Error)]
pub enum ImportError {
    #[error("no file")]
    NoFile,
    #[error("unreadable archive: {0}")]
    BadArchive(String),
    #[error("missing file {0}")]
    MissingFile(&'static str),
    #[error("unexpected content in {file}: {detail}")]
    Format { file: String, detail: String },
    #[error("no foods found")]
    NoFoods,
}

impl ImportError {
    /// User-facing explanation.
    pub fn message(&self, dataset: Dataset, lang: Lang) -> String {
        match self {
            ImportError::NoFile => lang.t("Choose the downloaded file to import.").into(),
            ImportError::BadArchive(_) => lang.f(
                "The file could not be opened. Upload the file exactly as downloaded for {0}.",
                &[&dataset.name()],
            ),
            ImportError::MissingFile(name) => lang.f(
                "The upload does not contain {0}. Check that you downloaded the right file for {1}.",
                &[name, &dataset.name()],
            ),
            ImportError::Format { file, .. } => lang.f(
                "{0} does not have the expected layout for {1}. The publisher may have changed its format.",
                &[file, &dataset.name()],
            ),
            ImportError::NoFoods => lang.f(
                "No foods were found in the upload for {0}.",
                &[&dataset.name()],
            ),
        }
    }
}

fn mask(list: &[Nutrient]) -> i64 {
    Nutrient::ALL
        .iter()
        .enumerate()
        .filter(|(_, n)| list.contains(n))
        .fold(0, |m, (i, _)| m | (1 << i))
}

fn unmask(mask: i64) -> Vec<Nutrient> {
    Nutrient::ALL
        .iter()
        .enumerate()
        .filter(|(i, _)| mask & (1 << i) != 0)
        .map(|(_, n)| *n)
        .collect()
}

/// Replaces every food of `dataset` in one transaction, so a failed import
/// leaves the previous version in place. A code listed twice keeps its first
/// row. `bundled_version` is the release when the foods come from a table
/// shipped with Korpus. Returns the number of foods stored.
pub async fn replace(
    db: &SqlitePool,
    dataset: Dataset,
    foods: &[DatasetFood],
    bundled_version: Option<&str>,
) -> AppResult<usize> {
    let mut seen = std::collections::HashSet::new();
    let foods: Vec<&DatasetFood> = foods.iter().filter(|f| seen.insert(&f.code)).collect();
    let mut tx = db.begin().await?;
    sqlx::query("DELETE FROM dataset_foods WHERE dataset = ?")
        .bind(dataset.id())
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO datasets (id, food_count, imported_at, bundled_version) VALUES (?, ?, ?, ?)
         ON CONFLICT (id) DO UPDATE SET food_count = excluded.food_count,
             imported_at = excluded.imported_at, bundled_version = excluded.bundled_version",
    )
    .bind(dataset.id())
    .bind(foods.len() as i64)
    .bind(now_utc())
    .bind(bundled_version)
    .execute(&mut *tx)
    .await?;
    for f in &foods {
        let n = &f.nutrients;
        let names: Vec<&str> = f.names.iter().map(|(_, n)| n.as_str()).collect();
        sqlx::query(
            "INSERT INTO dataset_foods (dataset, code, name, name_en, name_fr, name_es,
                 basis, energy_kcal, protein_g, carbs_g, fat_g, fiber_g, sugars_g, sat_fat_g,
                 salt_g, energy_from_kj, salt_from_sodium, carbs_include_fiber, trace_mask,
                 rejected_mask, search_text)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(dataset.id())
        .bind(&f.code)
        .bind(f.name())
        .bind(f.name_in(Lang::En))
        .bind(f.name_in(Lang::Fr))
        .bind(f.name_in(Lang::Es))
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
        .bind(f.salt_from_sodium)
        .bind(f.carbs_include_fiber)
        .bind(mask(&f.trace))
        .bind(mask(&f.rejected))
        .bind(crate::foods::search_text(&names, None, None))
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(foods.len())
}

/// Removes an imported table. Foods already copied into the user's foods
/// (and diary entries) are kept.
pub async fn remove(db: &SqlitePool, dataset: Dataset) -> AppResult<bool> {
    let res = sqlx::query("DELETE FROM datasets WHERE id = ?")
        .bind(dataset.id())
        .execute(db)
        .await?;
    Ok(res.rows_affected() > 0)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Installed {
    pub dataset: Dataset,
    pub food_count: i64,
    pub imported_at: String,
    /// The bundled release, when the table was not imported from a file.
    pub bundled_version: Option<String>,
}

pub async fn installed(db: &SqlitePool) -> AppResult<Vec<Installed>> {
    let rows: Vec<(String, i64, String, Option<String>)> = sqlx::query_as(
        "SELECT id, food_count, imported_at, bundled_version FROM datasets ORDER BY id",
    )
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|(id, food_count, imported_at, bundled_version)| {
            Some(Installed {
                dataset: Dataset::parse(&id)?,
                food_count,
                imported_at,
                bundled_version,
            })
        })
        .collect())
}

/// The table search uses for the profile's country, if any is imported.
pub async fn active(db: &SqlitePool, country: Option<&str>) -> AppResult<Option<Dataset>> {
    Ok(choose(country, &installed(db).await?))
}

#[derive(sqlx::FromRow)]
struct Row {
    code: String,
    name: String,
    name_en: Option<String>,
    name_fr: Option<String>,
    name_es: Option<String>,
    basis: String,
    energy_kcal: Option<f64>,
    protein_g: Option<f64>,
    carbs_g: Option<f64>,
    fat_g: Option<f64>,
    fiber_g: Option<f64>,
    sugars_g: Option<f64>,
    sat_fat_g: Option<f64>,
    salt_g: Option<f64>,
    energy_from_kj: bool,
    salt_from_sodium: bool,
    carbs_include_fiber: bool,
    trace_mask: i64,
    rejected_mask: i64,
}

const ROW_COLUMNS: &str = "code, name, name_en, name_fr, name_es, basis, energy_kcal, protein_g, \
    carbs_g, fat_g, fiber_g, sugars_g, sat_fat_g, salt_g, energy_from_kj, salt_from_sodium, \
    carbs_include_fiber, trace_mask, rejected_mask";

impl Row {
    fn into_food(self, dataset: Dataset, lang: Lang) -> ProviderFood {
        let names: Vec<(Lang, String)> = [
            (Lang::En, self.name_en),
            (Lang::Fr, self.name_fr),
            (Lang::Es, self.name_es),
        ]
        .into_iter()
        .filter_map(|(l, n)| Some((l, n?)))
        .collect();
        let name = names
            .iter()
            .find(|(l, _)| *l == lang)
            .map(|(_, n)| n.clone())
            .unwrap_or(self.name);
        ProviderFood {
            source: dataset.id(),
            provider_id: self.code,
            name,
            names,
            brand: None,
            barcode: None,
            basis: Basis::parse(&self.basis).unwrap_or(Basis::Per100g),
            basis_inferred: false,
            nutrients: Nutrients {
                energy_kcal: self.energy_kcal,
                protein_g: self.protein_g,
                carbs_g: self.carbs_g,
                fat_g: self.fat_g,
                fiber_g: self.fiber_g,
                sugars_g: self.sugars_g,
                sat_fat_g: self.sat_fat_g,
                salt_g: self.salt_g,
            },
            energy_from_kj: self.energy_from_kj,
            serving_amount: None,
            serving_label: None,
            source_updated_at: None,
            rejected: unmask(self.rejected_mask),
            salt_from_sodium: self.salt_from_sodium,
            carbs_include_fiber: self.carbs_include_fiber,
            trace_values: unmask(self.trace_mask),
        }
    }
}

pub struct SearchPage {
    pub foods: Vec<ProviderFood>,
    pub has_more: bool,
}

/// Searches one table, with names in `lang` where published. Foods the user
/// already saved from this table are left out: search lists the saved copy.
pub async fn search(
    db: &SqlitePool,
    dataset: Dataset,
    query: &str,
    page: usize,
    lang: Lang,
) -> AppResult<SearchPage> {
    let query = Query::new(query);
    let terms = query.retrieval_terms();
    if terms.is_empty() {
        return Ok(SearchPage {
            foods: Vec::new(),
            has_more: false,
        });
    }
    let sql = format!(
        "SELECT {ROW_COLUMNS} FROM dataset_foods d WHERE dataset = ?
             AND NOT EXISTS (SELECT 1 FROM foods f WHERE f.source = d.dataset
                             AND f.provider_id = d.code)
             AND {} LIMIT ?",
        // Every term starts a word: tables are large, and "eau" should not
        // fill the candidates with "gâteau" and "veau".
        vec!["(' ' || search_text) LIKE ? ESCAPE '\\'"; terms.len()].join(" AND ")
    );
    // Only constant SQL and `?` placeholders are concatenated; values are bound.
    let mut stmt = sqlx::query_as::<_, Row>(AssertSqlSafe(sql)).bind(dataset.id());
    for t in &terms {
        stmt = stmt.bind(format!("% {}%", crate::foods::escape_like(t)));
    }
    let rows = stmt.bind(SEARCH_CANDIDATES).fetch_all(db).await?;
    let mut foods: Vec<ProviderFood> = rows
        .into_iter()
        .map(|r| r.into_food(dataset, lang))
        .collect();
    // Among equally relevant foods, the shortest name is the plainest one.
    foods.sort_by(|a, b| (a.name.len(), &a.name).cmp(&(b.name.len(), &b.name)));
    // A food matched through its name in another language ranks a little
    // below one matched through the name shown.
    let foods = ranking::sort(&query, foods, |f| Candidate {
        name: &f.name,
        other_names: f
            .names
            .iter()
            .map(|(_, n)| n.as_str())
            .filter(|n| *n != f.name)
            .collect(),
        brands: &[],
        popularity: 0,
        // A table has no better-documented copy of a food to prefer, so a
        // value the table leaves blank is no reason to rank it lower.
        has_energy: true,
    });
    let offset = page.saturating_sub(1) * PAGE_SIZE;
    let has_more = foods.len() > offset + PAGE_SIZE;
    let foods = foods.into_iter().skip(offset).take(PAGE_SIZE).collect();
    Ok(SearchPage { foods, has_more })
}

/// One food of a table, with its name in `lang` where published.
pub async fn get(
    db: &SqlitePool,
    dataset: Dataset,
    code: &str,
    lang: Lang,
) -> AppResult<Option<ProviderFood>> {
    let row = sqlx::query_as::<_, Row>(AssertSqlSafe(format!(
        "SELECT {ROW_COLUMNS} FROM dataset_foods WHERE dataset = ? AND code = ?"
    )))
    .bind(dataset.id())
    .bind(code)
    .fetch_optional(db)
    .await?;
    Ok(row.map(|r| r.into_food(dataset, lang)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn food(code: &str, names: &[(Lang, &str)]) -> DatasetFood {
        DatasetFood {
            code: code.into(),
            names: names.iter().map(|(l, n)| (*l, n.to_string())).collect(),
            basis: Basis::Per100g,
            nutrients: Nutrients {
                energy_kcal: Some(52.0),
                protein_g: Some(0.3),
                carbs_g: Some(11.4),
                fat_g: Some(0.2),
                ..Default::default()
            },
            energy_from_kj: false,
            salt_from_sodium: true,
            carbs_include_fiber: false,
            trace: vec![Nutrient::Fat],
            rejected: vec![],
        }
    }

    fn imported(list: &[Dataset]) -> Vec<Installed> {
        list.iter()
            .map(|d| Installed {
                dataset: *d,
                food_count: 1,
                imported_at: String::new(),
                bundled_version: None,
            })
            .collect()
    }

    #[test]
    fn country_picks_its_own_table_then_the_closest() {
        let all = imported(&Dataset::ALL);
        assert_eq!(choose(Some("FR"), &all), Some(Dataset::Ciqual));
        assert_eq!(choose(Some("GB"), &all), Some(Dataset::Cofid));
        assert_eq!(choose(Some("US"), &all), Some(Dataset::Usda));
        assert_eq!(choose(Some("CA"), &all), Some(Dataset::Cnf));
        assert_eq!(choose(Some("NZ"), &all), Some(Dataset::Afcd));
        // A European country without its own table gets a European one.
        assert_eq!(choose(Some("ES"), &all), Some(Dataset::Ciqual));
        assert_eq!(choose(Some("JP"), &all), Some(Dataset::Usda));
        assert_eq!(choose(None, &all), Some(Dataset::Usda));

        let some = imported(&[Dataset::Usda, Dataset::Cofid]);
        assert_eq!(choose(Some("FR"), &some), Some(Dataset::Cofid));
        assert_eq!(choose(Some("CA"), &some), Some(Dataset::Usda));
        assert_eq!(choose(Some("FR"), &[]), None);
    }

    #[test]
    fn every_country_ranks_every_table_once() {
        for c in [
            None,
            Some("FR"),
            Some("GB"),
            Some("US"),
            Some("CA"),
            Some("AU"),
            Some("DE"),
        ] {
            let p = preference(c);
            for d in Dataset::ALL {
                assert_eq!(p.iter().filter(|x| **x == d).count(), 1, "{c:?} {d:?}");
            }
        }
    }

    #[test]
    fn nutrient_masks_round_trip() {
        let list = vec![Nutrient::Fat, Nutrient::Salt];
        assert_eq!(unmask(mask(&list)), list);
        assert_eq!(unmask(0), vec![]);
    }

    #[tokio::test]
    async fn stores_searches_and_replaces_a_table() {
        let db = crate::db::connect_memory().await.unwrap();
        let foods = vec![
            food("1", &[(Lang::Fr, "Pomme, crue"), (Lang::En, "Apple, raw")]),
            food(
                "2",
                &[(Lang::Fr, "Compote de pomme"), (Lang::En, "Apple sauce")],
            ),
            food("3", &[(Lang::Fr, "Poire, crue"), (Lang::En, "Pear, raw")]),
        ];
        let mut with_duplicate = foods.clone();
        with_duplicate.push(food("1", &[(Lang::Fr, "Doublon")]));
        assert_eq!(
            replace(&db, Dataset::Ciqual, &with_duplicate, None)
                .await
                .unwrap(),
            3
        );
        assert_eq!(
            active(&db, Some("FR")).await.unwrap(),
            Some(Dataset::Ciqual)
        );
        assert_eq!(installed(&db).await.unwrap()[0].food_count, 3);

        let page = search(&db, Dataset::Ciqual, "pomme", 1, Lang::Fr)
            .await
            .unwrap();
        let names: Vec<&str> = page.foods.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["Pomme, crue", "Compote de pomme"]);
        // Names follow the interface language; matching works in either.
        let page = search(&db, Dataset::Ciqual, "pomme", 1, Lang::En)
            .await
            .unwrap();
        assert_eq!(page.foods[0].name, "Apple, raw");
        let page = search(&db, Dataset::Ciqual, "apple", 1, Lang::Es)
            .await
            .unwrap();
        let names: Vec<&str> = page.foods.iter().map(|f| f.name.as_str()).collect();
        // No Spanish names: the main names are shown, ranked by the English match.
        assert_eq!(names, ["Pomme, crue", "Compote de pomme"]);
        let page = search(&db, Dataset::Ciqual, "raw", 1, Lang::Fr)
            .await
            .unwrap();
        let names: Vec<&str> = page.foods.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["Poire, crue", "Pomme, crue"]);

        let apple = get(&db, Dataset::Ciqual, "1", Lang::En)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(apple.source, "ciqual");
        assert_eq!(apple.nutrients.energy_kcal, Some(52.0));
        assert!(apple.salt_from_sodium);
        assert_eq!(apple.trace_values, vec![Nutrient::Fat]);
        let notes = apple.import_notes(Lang::En).unwrap();
        assert!(
            notes.contains("sodium") && notes.contains("traces"),
            "{notes}"
        );

        // Importing again replaces the whole table.
        replace(&db, Dataset::Ciqual, &foods[2..], None)
            .await
            .unwrap();
        assert!(
            get(&db, Dataset::Ciqual, "1", Lang::En)
                .await
                .unwrap()
                .is_none()
        );
        assert!(remove(&db, Dataset::Ciqual).await.unwrap());
        assert_eq!(active(&db, Some("FR")).await.unwrap(), None);
        let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM dataset_foods")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(left, 0, "removing a table removes its foods");
    }

    /// Generic foods typed the way people search for them, against the
    /// Ciqual rows they rank highest. The previous ranking put an
    /// acceptable food first for 34 of 50 of these queries.
    #[tokio::test]
    async fn ranks_generic_foods_first() {
        let path = format!(
            "{}/tests/fixtures/ciqual_ranking.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let fixture: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let foods: Vec<DatasetFood> = fixture["foods"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| {
                let mut names = vec![(Lang::Fr, f["fr"].as_str().unwrap())];
                names.extend(f["en"].as_str().map(|n| (Lang::En, n)));
                food(f["code"].as_str().unwrap(), &names)
            })
            .collect();
        let db = crate::db::connect_memory().await.unwrap();
        replace(&db, Dataset::Ciqual, &foods, None).await.unwrap();

        let queries = fixture["queries"].as_array().unwrap();
        let (mut top1, mut top5) = (0, 0);
        let mut misses = Vec::new();
        for entry in queries {
            let query = entry["query"].as_str().unwrap();
            let expect: Vec<&str> = entry["expect"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| e.as_str().unwrap())
                .collect();
            let page = search(&db, Dataset::Ciqual, query, 1, Lang::Fr)
                .await
                .unwrap();
            let pos = page
                .foods
                .iter()
                .take(5)
                .position(|f| expect.iter().any(|e| f.name.starts_with(e)));
            top1 += usize::from(pos == Some(0));
            top5 += usize::from(pos.is_some());
            if pos != Some(0) {
                misses.push(format!("{query}: {pos:?} (top: {})", page.foods[0].name));
            }
        }
        let n = queries.len();
        eprintln!("top-1 {top1}/{n}, top-5 {top5}/{n}; misses: {misses:#?}");
        assert!(
            top1 >= 46,
            "top-1 accuracy dropped to {top1}/{n}: {misses:#?}"
        );
        assert!(
            top5 >= 50,
            "top-5 recall dropped to {top5}/{n}: {misses:#?}"
        );
    }
}
