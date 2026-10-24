//! The single local profile: setup state, units, time zone, language,
//! country and daily step goal.

use sqlx::SqlitePool;

use crate::error::AppResult;
use crate::i18n::Lang;
use crate::units::UnitSystem;

#[derive(Clone, Debug, sqlx::FromRow, serde::Serialize, serde::Deserialize)]
pub struct Profile {
    pub setup_complete: bool,
    pub unit_system: String,
    pub timezone: String,
    /// Interface language code; `None` follows the browser. Absent in
    /// exports made before languages existed.
    #[serde(default)]
    pub language: Option<String>,
    /// ISO 3166-1 alpha-2 code; picks the local food composition table.
    /// Absent in exports made before countries existed.
    #[serde(default)]
    pub country: Option<String>,
    /// Steps that fill the steps ring. Absent in exports made before step
    /// goals existed.
    #[serde(default = "default_step_goal")]
    pub step_goal: i64,
}

fn default_step_goal() -> i64 {
    crate::activity::DEFAULT_STEP_GOAL
}

impl Profile {
    pub fn units(&self) -> UnitSystem {
        UnitSystem::parse(&self.unit_system).unwrap_or(UnitSystem::Metric)
    }

    pub fn lang(&self) -> Option<Lang> {
        self.language.as_deref().and_then(Lang::parse)
    }

    pub fn country(&self) -> Option<&'static str> {
        self.country.as_deref().and_then(crate::countries::parse)
    }
}

pub async fn load(db: &SqlitePool) -> AppResult<Profile> {
    Ok(sqlx::query_as::<_, Profile>(
        "SELECT setup_complete, unit_system, timezone, language, country, step_goal
         FROM profile WHERE id = 1",
    )
    .fetch_one(db)
    .await?)
}

/// The chosen interface language, if any. Used on every page request, so it
/// reads only this one column.
pub async fn language(db: &SqlitePool) -> Result<Option<Lang>, sqlx::Error> {
    let code: Option<String> = sqlx::query_scalar("SELECT language FROM profile WHERE id = 1")
        .fetch_one(db)
        .await?;
    Ok(code.as_deref().and_then(Lang::parse))
}

pub async fn save(
    db: &SqlitePool,
    units: UnitSystem,
    timezone: &str,
    language: Option<Lang>,
    country: Option<&str>,
) -> AppResult<()> {
    sqlx::query(
        "UPDATE profile SET setup_complete = 1, unit_system = ?, timezone = ?, language = ?,
             country = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
         WHERE id = 1",
    )
    .bind(units.as_str())
    .bind(timezone)
    .bind(language.map(Lang::code))
    .bind(country)
    .execute(db)
    .await?;
    Ok(())
}

/// The chosen country, if any. Read on every search, so only this column.
pub async fn country(db: &SqlitePool) -> Result<Option<&'static str>, sqlx::Error> {
    let code: Option<String> = sqlx::query_scalar("SELECT country FROM profile WHERE id = 1")
        .fetch_one(db)
        .await?;
    Ok(code.as_deref().and_then(crate::countries::parse))
}
