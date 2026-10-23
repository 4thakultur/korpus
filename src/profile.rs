//! The single local profile: setup state, units, time zone and language.

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
}

impl Profile {
    pub fn units(&self) -> UnitSystem {
        UnitSystem::parse(&self.unit_system).unwrap_or(UnitSystem::Metric)
    }

    pub fn lang(&self) -> Option<Lang> {
        self.language.as_deref().and_then(Lang::parse)
    }
}

pub async fn load(db: &SqlitePool) -> AppResult<Profile> {
    Ok(sqlx::query_as::<_, Profile>(
        "SELECT setup_complete, unit_system, timezone, language FROM profile WHERE id = 1",
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
) -> AppResult<()> {
    sqlx::query(
        "UPDATE profile SET setup_complete = 1, unit_system = ?, timezone = ?, language = ?,
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
         WHERE id = 1",
    )
    .bind(units.as_str())
    .bind(timezone)
    .bind(language.map(Lang::code))
    .execute(db)
    .await?;
    Ok(())
}
