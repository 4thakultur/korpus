//! Export, import and consistent SQLite backups.

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::db::now_utc;
use crate::diary::{Entry, Meal, check_amount};
use crate::error::{AppError, AppResult};
use crate::foods::{Food, search_text};
use crate::goals::GoalSettings;
use crate::i18n::Lang;
use crate::nutrition::{Basis, Nutrient, check_value};
use crate::profile::Profile;
use crate::units::{UnitSystem, parse_date, valid_timezone};
use crate::weight::{WeightEntry, check_weight_kg};

pub const EXPORT_FORMAT: &str = "korpus-export";
pub const EXPORT_VERSION: u32 = 1;
/// Upper bound for uploaded import files.
pub const MAX_IMPORT_BYTES: usize = 32 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
pub struct Export {
    pub format: String,
    pub version: u32,
    pub exported_at: String,
    pub profile: Profile,
    pub goals: GoalSettings,
    pub foods: Vec<Food>,
    pub diary_entries: Vec<Entry>,
    pub weight_entries: Vec<WeightEntry>,
}

pub async fn export(db: &SqlitePool) -> AppResult<Export> {
    let foods = crate::foods::all_stored(db).await?;
    let diary_entries = sqlx::query_as::<_, Entry>(
        "SELECT id, diary_date, meal, food_id, amount, unit, food_name, food_brand, source,
                energy_kcal, protein_g, carbs_g, fat_g, fiber_g, sugars_g, sat_fat_g, salt_g
         FROM diary_entries ORDER BY diary_date, id",
    )
    .fetch_all(db)
    .await?;
    let weight_entries = sqlx::query_as::<_, WeightEntry>(
        "SELECT id, entry_date, weight_kg, note FROM weight_entries ORDER BY entry_date",
    )
    .fetch_all(db)
    .await?;
    Ok(Export {
        format: EXPORT_FORMAT.into(),
        version: EXPORT_VERSION,
        exported_at: now_utc(),
        profile: crate::profile::load(db).await?,
        goals: crate::goals::load(db).await?,
        foods,
        diary_entries,
        weight_entries,
    })
}

/// Prevents spreadsheet applications from interpreting a cell as a formula.
fn csv_field(s: &str) -> String {
    let guarded = if s.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{s}")
    } else {
        s.to_string()
    };
    if guarded.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", guarded.replace('"', "\"\""))
    } else {
        guarded
    }
}

fn csv_num(v: Option<f64>) -> String {
    v.map(|v| format!("{v:.2}")).unwrap_or_default()
}

/// Diary entries as CSV, one row per entry, with nutrients for the amount eaten.
pub fn diary_csv(entries: &[Entry]) -> String {
    let mut out = String::from(
        "date,meal,food,brand,amount,unit,energy_kcal,protein_g,carbs_g,fat_g,fiber_g,sugars_g,saturated_fat_g,salt_g,source\n",
    );
    for e in entries {
        let n = e.nutrients();
        let row = [
            csv_field(&e.diary_date),
            csv_field(&e.meal),
            csv_field(&e.food_name),
            csv_field(e.food_brand.as_deref().unwrap_or("")),
            format!("{}", e.amount),
            csv_field(&e.unit),
            csv_num(n.energy_kcal),
            csv_num(n.protein_g),
            csv_num(n.carbs_g),
            csv_num(n.fat_g),
            csv_num(n.fiber_g),
            csv_num(n.sugars_g),
            csv_num(n.sat_fat_g),
            csv_num(n.salt_g),
            csv_field(&e.source),
        ];
        out.push_str(&row.join(","));
        out.push('\n');
    }
    out
}

/// Writes a consistent snapshot of the database with `VACUUM INTO`, which is
/// safe while the application keeps writing, and returns its bytes.
pub async fn sqlite_snapshot(db: &SqlitePool) -> AppResult<Vec<u8>> {
    let name = format!(
        "korpus-backup-{}-{}.db",
        std::process::id(),
        jiff::Timestamp::now().as_nanosecond()
    );
    let path = std::env::temp_dir().join(name);
    let path_s = path
        .to_str()
        .ok_or_else(|| AppError::Internal("temporary path is not valid UTF-8".into()))?
        .to_string();
    sqlx::query("VACUUM INTO ?")
        .bind(&path_s)
        .execute(db)
        .await?;
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|e| AppError::Internal(format!("reading backup: {e}")));
    if let Err(e) = tokio::fs::remove_file(&path).await {
        tracing::warn!(error = %e, "could not remove temporary backup file");
    }
    bytes
}

#[derive(Debug, PartialEq)]
pub struct ImportSummary {
    pub foods: usize,
    pub diary_entries: usize,
    pub weight_entries: usize,
}

fn invalid(lang: Lang, what: impl std::fmt::Display) -> AppError {
    AppError::BadRequest(lang.f("The import file is invalid: {0}.", &[&what]))
}

/// "<record>: <problem>", e.g. "food 3: name".
fn at(record: String, problem: impl std::fmt::Display) -> String {
    format!("{record}: {problem}")
}

fn check_nutrients(
    basis: Basis,
    values: [(Nutrient, Option<f64>); 8],
    ctx: &str,
    lang: Lang,
) -> AppResult<()> {
    for (n, v) in values {
        if let Some(v) = v {
            check_value(n, basis, v).map_err(|e| {
                invalid(
                    lang,
                    format!("{ctx}: {} {}", n.label(lang), e.message(lang)),
                )
            })?;
        }
    }
    Ok(())
}

fn validate(export: &Export, lang: Lang) -> AppResult<()> {
    if export.format != EXPORT_FORMAT {
        return Err(invalid(lang, lang.t("this is not a Korpus export")));
    }
    if export.version != EXPORT_VERSION {
        return Err(invalid(
            lang,
            lang.f("unsupported export version {0}", &[&export.version]),
        ));
    }
    if UnitSystem::parse(&export.profile.unit_system).is_none() {
        return Err(invalid(lang, lang.t("unknown unit system")));
    }
    if !valid_timezone(&export.profile.timezone) {
        return Err(invalid(lang, lang.t("unknown time zone")));
    }
    if export.profile.language.is_some() && export.profile.lang().is_none() {
        return Err(invalid(lang, lang.t("unknown language")));
    }
    if export.profile.country.is_some() && export.profile.country().is_none() {
        return Err(invalid(lang, lang.t("unknown country")));
    }
    for f in &export.foods {
        let food = || lang.f("food {0}", &[&f.id]);
        let basis =
            Basis::parse(&f.basis).ok_or_else(|| invalid(lang, at(food(), lang.t("basis"))))?;
        let bad_name = |n: &str| n.trim().is_empty() || n.chars().count() > 200;
        if bad_name(&f.name) {
            return Err(invalid(lang, at(food(), lang.t("name"))));
        }
        for n in &f.names {
            if Lang::parse(&n.lang).is_none() || bad_name(&n.name) {
                return Err(invalid(lang, at(food(), lang.t("name"))));
            }
        }
        let n = f.nutrients();
        check_nutrients(basis, Nutrient::ALL.map(|k| (k, n.get(k))), &food(), lang)?;
    }
    for e in &export.diary_entries {
        let ctx = lang.f("diary entry {0}", &[&e.id]);
        parse_date(&e.diary_date).ok_or_else(|| invalid(lang, at(ctx.clone(), lang.t("date"))))?;
        Meal::parse(&e.meal).ok_or_else(|| invalid(lang, at(ctx.clone(), lang.t("meal"))))?;
        let basis =
            Basis::parse(&e.unit).ok_or_else(|| invalid(lang, at(ctx.clone(), lang.t("unit"))))?;
        check_amount(e.amount, lang).map_err(|m| invalid(lang, at(ctx.clone(), m)))?;
        let n = e.per_100();
        check_nutrients(basis, Nutrient::ALL.map(|k| (k, n.get(k))), &ctx, lang)?;
    }
    for w in &export.weight_entries {
        let ctx = || lang.f("weight {0}", &[&w.id]);
        parse_date(&w.entry_date).ok_or_else(|| invalid(lang, at(ctx(), lang.t("date"))))?;
        check_weight_kg(w.weight_kg, lang).map_err(|m| invalid(lang, at(ctx(), m)))?;
    }
    Ok(())
}

/// Restores an export into an installation that has no foods, diary entries
/// or weights yet. Refusing to merge into existing data avoids duplicates
/// and ambiguous conflicts. Everything happens in one transaction.
pub async fn import(db: &SqlitePool, export: &Export, lang: Lang) -> AppResult<ImportSummary> {
    validate(export, lang)?;

    let mut tx = db.begin().await?;
    let existing: i64 = sqlx::query_scalar(
        "SELECT (SELECT COUNT(*) FROM foods) + (SELECT COUNT(*) FROM diary_entries)
              + (SELECT COUNT(*) FROM weight_entries)",
    )
    .fetch_one(&mut *tx)
    .await?;
    if existing > 0 {
        return Err(AppError::BadRequest(
            lang.t("Import is only possible into an empty installation. This one already contains foods, diary entries or weights.")
                .into(),
        ));
    }

    let p = &export.profile;
    sqlx::query(
        "UPDATE profile SET setup_complete = 1, unit_system = ?, timezone = ?, language = ?,
             country = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = 1",
    )
    .bind(&p.unit_system)
    .bind(&p.timezone)
    .bind(p.lang().map(Lang::code))
    .bind(p.country())
    .execute(&mut *tx)
    .await?;

    let g = &export.goals;
    sqlx::query(
        "UPDATE goal_settings SET daily_kcal_target = ?, target_source = ?, goal = ?, sex = ?,
             birth_date = ?, height_cm = ?, weight_kg = ?, activity = ?, special_circumstances = ?,
             maintenance_kcal = ?, calculation_version = ?, protein_goal_g = ?, carbs_goal_g = ?,
             fat_goal_g = ?, updated_at = ? WHERE id = 1",
    )
    .bind(g.daily_kcal_target)
    .bind(&g.target_source)
    .bind(&g.goal)
    .bind(&g.sex)
    .bind(&g.birth_date)
    .bind(g.height_cm)
    .bind(g.weight_kg)
    .bind(&g.activity)
    .bind(g.special_circumstances)
    .bind(g.maintenance_kcal)
    .bind(g.calculation_version)
    .bind(g.protein_goal_g)
    .bind(g.carbs_goal_g)
    .bind(g.fat_goal_g)
    .bind(&g.updated_at)
    .execute(&mut *tx)
    .await?;

    // Ids are preserved, so diary references stay valid without remapping.
    for f in &export.foods {
        let names: Vec<&str> = std::iter::once(f.name.as_str())
            .chain(f.names.iter().map(|n| n.name.as_str()))
            .collect();
        let text = search_text(&names, f.brand.as_deref(), f.barcode.as_deref());
        sqlx::query(
            "INSERT INTO foods (id, name, brand, barcode, basis, energy_kcal, protein_g, carbs_g,
                 fat_g, fiber_g, sugars_g, sat_fat_g, salt_g, energy_from_kj, serving_amount,
                 serving_label, notes, source, provider_id, source_updated_at, fetched_at,
                 edited_locally, search_text, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(f.id)
        .bind(&f.name)
        .bind(&f.brand)
        .bind(&f.barcode)
        .bind(&f.basis)
        .bind(f.energy_kcal)
        .bind(f.protein_g)
        .bind(f.carbs_g)
        .bind(f.fat_g)
        .bind(f.fiber_g)
        .bind(f.sugars_g)
        .bind(f.sat_fat_g)
        .bind(f.salt_g)
        .bind(f.energy_from_kj)
        .bind(f.serving_amount)
        .bind(&f.serving_label)
        .bind(&f.notes)
        .bind(&f.source)
        .bind(&f.provider_id)
        .bind(&f.source_updated_at)
        .bind(&f.fetched_at)
        .bind(f.edited_locally)
        .bind(text)
        .bind(&f.created_at)
        .bind(&f.updated_at)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            invalid(
                lang,
                lang.f("food {0} could not be stored ({1})", &[&f.id, &e]),
            )
        })?;
        for n in &f.names {
            sqlx::query("INSERT INTO food_names (food_id, lang, name) VALUES (?, ?, ?)")
                .bind(f.id)
                .bind(&n.lang)
                .bind(&n.name)
                .execute(&mut *tx)
                .await
                .map_err(|e| {
                    invalid(
                        lang,
                        lang.f("food {0} could not be stored ({1})", &[&f.id, &e]),
                    )
                })?;
        }
    }

    for e in &export.diary_entries {
        sqlx::query(
            "INSERT INTO diary_entries (diary_date, meal, food_id, amount, unit, food_name,
                 food_brand, source, energy_kcal, protein_g, carbs_g, fat_g, fiber_g, sugars_g,
                 sat_fat_g, salt_g)
             VALUES (?, ?, (SELECT id FROM foods WHERE id = ?), ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&e.diary_date)
        .bind(&e.meal)
        .bind(e.food_id)
        .bind(e.amount)
        .bind(&e.unit)
        .bind(&e.food_name)
        .bind(&e.food_brand)
        .bind(&e.source)
        .bind(e.energy_kcal)
        .bind(e.protein_g)
        .bind(e.carbs_g)
        .bind(e.fat_g)
        .bind(e.fiber_g)
        .bind(e.sugars_g)
        .bind(e.sat_fat_g)
        .bind(e.salt_g)
        .execute(&mut *tx)
        .await?;
    }

    for w in &export.weight_entries {
        sqlx::query("INSERT INTO weight_entries (entry_date, weight_kg, note) VALUES (?, ?, ?)")
            .bind(&w.entry_date)
            .bind(w.weight_kg)
            .bind(&w.note)
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                invalid(
                    lang,
                    lang.f(
                        "weight for {0} could not be stored ({1})",
                        &[&w.entry_date, &e],
                    ),
                )
            })?;
    }

    tx.commit().await?;
    Ok(ImportSummary {
        foods: export.foods.len(),
        diary_entries: export.diary_entries.len(),
        weight_entries: export.weight_entries.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_quotes_and_guards_formulas() {
        assert_eq!(csv_field("Oats"), "Oats");
        assert_eq!(csv_field("Oats, rolled"), "\"Oats, rolled\"");
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_field("=SUM(A1)"), "'=SUM(A1)");
        assert_eq!(csv_field("-1"), "'-1");
    }
}
