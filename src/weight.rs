//! Weight history, stored canonically in kilograms.

use jiff::civil::Date;
use sqlx::SqlitePool;

use crate::error::AppResult;
use crate::i18n::Lang;

/// Plausible human body weight range, in kg.
pub const MIN_KG: f64 = 20.0;
pub const MAX_KG: f64 = 400.0;
const HISTORY_LIMIT: i64 = 366;

#[derive(Clone, Debug, sqlx::FromRow, serde::Serialize, serde::Deserialize)]
pub struct WeightEntry {
    pub id: i64,
    pub entry_date: String,
    pub weight_kg: f64,
    pub note: Option<String>,
}

pub fn check_weight_kg(kg: f64, lang: Lang) -> Result<f64, String> {
    if !kg.is_finite() {
        return Err(lang.t("Enter a number.").into());
    }
    if !(MIN_KG..=MAX_KG).contains(&kg) {
        return Err(lang.f(
            "Enter a weight between {0} and {1} kg (or the equivalent in lb).",
            &[&MIN_KG, &MAX_KG],
        ));
    }
    Ok(kg)
}

/// Most recent entries first, bounded.
pub async fn history(db: &SqlitePool) -> AppResult<Vec<WeightEntry>> {
    Ok(sqlx::query_as::<_, WeightEntry>(
        "SELECT id, entry_date, weight_kg, note FROM weight_entries
         ORDER BY entry_date DESC LIMIT ?",
    )
    .bind(HISTORY_LIMIT)
    .fetch_all(db)
    .await?)
}

pub async fn latest(db: &SqlitePool) -> AppResult<Option<WeightEntry>> {
    Ok(sqlx::query_as::<_, WeightEntry>(
        "SELECT id, entry_date, weight_kg, note FROM weight_entries
         ORDER BY entry_date DESC LIMIT 1",
    )
    .fetch_optional(db)
    .await?)
}

pub async fn get(db: &SqlitePool, id: i64) -> AppResult<Option<WeightEntry>> {
    Ok(sqlx::query_as::<_, WeightEntry>(
        "SELECT id, entry_date, weight_kg, note FROM weight_entries WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(db)
    .await?)
}

#[derive(Debug, PartialEq)]
pub enum Saved {
    Created,
    Replaced,
}

/// Records the weight for a date, replacing an existing value for that date.
pub async fn record(db: &SqlitePool, date: Date, kg: f64, note: Option<&str>) -> AppResult<Saved> {
    let existed: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM weight_entries WHERE entry_date = ?)")
            .bind(date.to_string())
            .fetch_one(db)
            .await?;
    sqlx::query(
        "INSERT INTO weight_entries (entry_date, weight_kg, note) VALUES (?, ?, ?)
         ON CONFLICT (entry_date) DO UPDATE SET weight_kg = excluded.weight_kg,
             note = excluded.note, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')",
    )
    .bind(date.to_string())
    .bind(kg)
    .bind(note)
    .execute(db)
    .await?;
    Ok(if existed {
        Saved::Replaced
    } else {
        Saved::Created
    })
}

#[derive(Debug, PartialEq)]
pub enum UpdateResult {
    Updated,
    NotFound,
    DateTaken,
}

pub async fn update(
    db: &SqlitePool,
    id: i64,
    date: Date,
    kg: f64,
    note: Option<&str>,
) -> AppResult<UpdateResult> {
    let taken: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM weight_entries WHERE entry_date = ? AND id <> ?)",
    )
    .bind(date.to_string())
    .bind(id)
    .fetch_one(db)
    .await?;
    if taken {
        return Ok(UpdateResult::DateTaken);
    }
    let res = sqlx::query(
        "UPDATE weight_entries SET entry_date = ?, weight_kg = ?, note = ?,
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
         WHERE id = ?",
    )
    .bind(date.to_string())
    .bind(kg)
    .bind(note)
    .bind(id)
    .execute(db)
    .await?;
    Ok(if res.rows_affected() > 0 {
        UpdateResult::Updated
    } else {
        UpdateResult::NotFound
    })
}

pub async fn delete(db: &SqlitePool, id: i64) -> AppResult<bool> {
    let res = sqlx::query("DELETE FROM weight_entries WHERE id = ?")
        .bind(id)
        .execute(db)
        .await?;
    Ok(res.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Lang::En;

    #[test]
    fn weight_bounds() {
        assert_eq!(check_weight_kg(72.5, En), Ok(72.5));
        assert!(check_weight_kg(0.0, En).is_err());
        assert!(check_weight_kg(-70.0, En).is_err());
        assert!(check_weight_kg(f64::INFINITY, En).is_err());
        assert!(check_weight_kg(1000.0, En).is_err());
    }
}
