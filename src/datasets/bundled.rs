//! Food composition tables shipped with Korpus, so generic foods can be
//! found on a fresh installation without downloading anything.
//!
//! Only tables whose licence allows redistribution are bundled; the
//! extract, its source and its licence are described in `bundled/`.

use sqlx::SqlitePool;

use super::{Dataset, DatasetFood, ImportError, ciqual};
use crate::error::{AppError, AppResult};

pub struct Bundled {
    pub dataset: Dataset,
    /// The publisher's release date, stored in `datasets.bundled_version`.
    pub version: &'static str,
    /// The release as the publisher names it.
    pub label: &'static str,
    text: &'static str,
}

pub const BUNDLED: &[Bundled] = &[Bundled {
    dataset: Dataset::Ciqual,
    version: "2025-11-03",
    label: "Ciqual 2025",
    text: include_str!("../../bundled/ciqual/ciqual_2025_11_03.tsv"),
}];

impl Bundled {
    pub fn get(dataset: Dataset) -> Option<&'static Bundled> {
        BUNDLED.iter().find(|b| b.dataset == dataset)
    }

    pub fn foods(&self) -> Result<Vec<DatasetFood>, ImportError> {
        match self.dataset {
            Dataset::Ciqual => ciqual::parse_extract(self.text),
            _ => Err(ImportError::NoFoods),
        }
    }

    /// Installs this release, replacing the table, and records it as
    /// offered. CPU-bound parsing runs on a blocking thread.
    pub async fn install(&'static self, db: &SqlitePool) -> AppResult<usize> {
        let foods = tokio::task::spawn_blocking(|| self.foods())
            .await
            .map_err(|e| AppError::Internal(format!("bundled table task: {e}")))?
            .map_err(|e| AppError::Internal(format!("bundled {}: {e}", self.dataset.id())))?;
        let stored = super::replace(db, self.dataset, &foods, Some(self.version)).await?;
        self.mark_offered(db).await?;
        Ok(stored)
    }

    async fn mark_offered(&self, db: &SqlitePool) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO bundled_datasets (id, version) VALUES (?, ?)
             ON CONFLICT (id) DO UPDATE SET version = excluded.version",
        )
        .bind(self.dataset.id())
        .bind(self.version)
        .execute(db)
        .await?;
        Ok(())
    }
}

/// What startup should do with one bundled release.
#[derive(Debug, PartialEq)]
enum Step {
    Install,
    /// Leave the table as it is, remembering the release was offered.
    Keep,
    Nothing,
}

/// `offered`: the release last offered; `installed`: `None` when the table
/// is not imported, `Some(None)` when the administrator imported it from
/// the publisher's file, `Some(Some(version))` when it is a bundled release.
fn step(version: &str, offered: Option<&str>, installed: Option<Option<&str>>) -> Step {
    match (offered, installed) {
        (Some(v), _) if v == version => Step::Nothing,
        (_, Some(None)) => Step::Keep,
        // Removed after an earlier release was installed.
        (Some(_), None) => Step::Nothing,
        _ => Step::Install,
    }
}

/// Installs bundled releases not yet offered to this installation. Runs at
/// startup; a failure is logged and leaves the existing tables in place.
pub async fn install_new(db: &SqlitePool) -> AppResult<()> {
    for b in BUNDLED {
        let offered: Option<String> =
            sqlx::query_scalar("SELECT version FROM bundled_datasets WHERE id = ?")
                .bind(b.dataset.id())
                .fetch_optional(db)
                .await?;
        let installed: Option<Option<String>> =
            sqlx::query_scalar("SELECT bundled_version FROM datasets WHERE id = ?")
                .bind(b.dataset.id())
                .fetch_optional(db)
                .await?;
        match step(
            b.version,
            offered.as_deref(),
            installed.as_ref().map(Option::as_deref),
        ) {
            Step::Install => {
                let stored = b.install(db).await?;
                tracing::info!(
                    dataset = b.dataset.id(),
                    version = b.version,
                    foods = stored,
                    "bundled food table installed"
                );
            }
            Step::Keep => b.mark_offered(db).await?,
            Step::Nothing => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_installs_each_release_once_and_respects_the_user() {
        // Fresh installation, and a newer release replacing a bundled one.
        assert_eq!(step("2025", None, None), Step::Install);
        assert_eq!(
            step("2026", Some("2025"), Some(Some("2025"))),
            Step::Install
        );
        // Already done.
        assert_eq!(
            step("2025", Some("2025"), Some(Some("2025"))),
            Step::Nothing
        );
        assert_eq!(step("2025", Some("2025"), None), Step::Nothing);
        // The administrator's own import is never replaced.
        assert_eq!(step("2025", None, Some(None)), Step::Keep);
        assert_eq!(step("2026", Some("2025"), Some(None)), Step::Keep);
        // Removed by the user: a newer release does not bring it back.
        assert_eq!(step("2026", Some("2025"), None), Step::Nothing);
    }

    #[test]
    fn bundled_extracts_parse() {
        for b in BUNDLED {
            let foods = b.foods().unwrap();
            assert!(foods.len() > 3000, "{} foods", foods.len());
            let with_energy = foods.iter().filter(|f| f.nutrients.energy_kcal.is_some());
            assert!(with_energy.count() > 3000);
        }
    }

    #[tokio::test]
    async fn installs_on_a_fresh_database_and_keeps_a_removal() {
        let db = crate::db::connect_memory().await.unwrap();
        install_new(&db).await.unwrap();
        let installed = super::super::installed(&db).await.unwrap();
        assert_eq!(installed.len(), 1);
        assert_eq!(installed[0].dataset, Dataset::Ciqual);
        assert_eq!(installed[0].bundled_version.as_deref(), Some("2025-11-03"));
        let page = super::super::search(&db, Dataset::Ciqual, "oeuf", 1, crate::i18n::Lang::Fr)
            .await
            .unwrap();
        assert!(
            page.foods
                .iter()
                .any(|f| f.name.starts_with("Oeuf") || f.name.starts_with("Œuf"))
        );

        super::super::remove(&db, Dataset::Ciqual).await.unwrap();
        install_new(&db).await.unwrap();
        assert!(super::super::installed(&db).await.unwrap().is_empty());

        // Installing it again by hand is always possible.
        BUNDLED[0].install(&db).await.unwrap();
        assert_eq!(super::super::installed(&db).await.unwrap().len(), 1);
    }

    /// Basic foods typed the way people search for them, against the
    /// bundled table: the plain food comes first, without picking a brand.
    #[tokio::test]
    async fn basic_foods_come_first() {
        let db = crate::db::connect_memory().await.unwrap();
        BUNDLED[0].install(&db).await.unwrap();
        let cases: &[(&str, crate::i18n::Lang, &[&str])] = {
            use crate::i18n::Lang::{En, Fr};
            &[
                ("pomme", Fr, &["Pomme, chair"]),
                ("banane", Fr, &["Banane, chair"]),
                ("poire", Fr, &["Poire, chair"]),
                ("orange", Fr, &["Orange, chair"]),
                ("kiwi", Fr, &["Kiwi, chair"]),
                ("oeuf", Fr, &["Oeuf cru", "Oeuf, cru"]),
                ("oeufs", Fr, &["Oeuf cru", "Oeuf, cru"]),
                (
                    "lait",
                    Fr,
                    &[
                        "Lait entier (aliment moyen)",
                        "Lait demi-écrémé (aliment moyen)",
                    ],
                ),
                (
                    "lait demi écrémé",
                    Fr,
                    &["Lait demi-écrémé (aliment moyen)"],
                ),
                (
                    "carotte",
                    Fr,
                    &["Carotte, crue", "Carotte, cuite (aliment moyen)"],
                ),
                (
                    "tomate",
                    Fr,
                    &["Tomate sans précision, crue", "Tomate, crue"],
                ),
                (
                    "riz",
                    Fr,
                    &["Riz blanc, cuit", "Riz, cuit", "Riz blanc, cru"],
                ),
                ("apple", En, &["Apple, flesh"]),
                ("banana", En, &["Banana, flesh"]),
                ("egg", En, &["Egg, raw"]),
                ("milk", En, &["Milk, whole", "Milk, semi-skimmed"]),
            ]
        };
        let mut misses = Vec::new();
        for (q, lang, expect) in cases {
            let page = super::super::search(&db, Dataset::Ciqual, q, 1, *lang)
                .await
                .unwrap();
            let first = page.foods.first().map(|f| f.name.as_str()).unwrap_or("");
            if !expect.iter().any(|e| first.starts_with(e)) {
                let top: Vec<&str> = page.foods.iter().take(3).map(|f| f.name.as_str()).collect();
                misses.push(format!("{q}: {top:?}"));
            }
        }
        assert!(misses.is_empty(), "{misses:#?}");
    }
}
