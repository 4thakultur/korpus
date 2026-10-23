//! Diary entries, meals and daily totals.
//!
//! Every entry stores a snapshot of the food's name and per-100 nutrition at
//! the time it was logged. Totals are always derived from these snapshots,
//! so editing or deleting a food later never changes past days. Only the
//! displayed name follows the interface language: when the food has an
//! official name in that language, it is shown instead of the snapshot.

use jiff::civil::Date;
use sqlx::{AssertSqlSafe, SqlitePool};

use crate::error::AppResult;
use crate::foods::Food;
use crate::i18n::Lang;
use crate::nutrition::{Basis, Nutrients, Totals};

/// Largest single amount accepted, in g or ml.
pub const MAX_AMOUNT: f64 = 10_000.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Meal {
    Breakfast,
    Lunch,
    Dinner,
    Snacks,
}

impl Meal {
    pub const ALL: [Meal; 4] = [Meal::Breakfast, Meal::Lunch, Meal::Dinner, Meal::Snacks];

    pub fn as_str(self) -> &'static str {
        match self {
            Meal::Breakfast => "breakfast",
            Meal::Lunch => "lunch",
            Meal::Dinner => "dinner",
            Meal::Snacks => "snacks",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Meal::ALL.into_iter().find(|m| m.as_str() == s)
    }

    pub fn label(self, lang: Lang) -> &'static str {
        match self {
            Meal::Breakfast => lang.t("Breakfast"),
            Meal::Lunch => lang.t("Lunch"),
            Meal::Dinner => lang.t("Dinner"),
            Meal::Snacks => lang.t("Snacks"),
        }
    }

    /// Link text for adding to this meal. Whole phrases per meal, because
    /// articles and prepositions differ by meal in French and Spanish.
    pub fn add_label(self, lang: Lang) -> &'static str {
        match self {
            Meal::Breakfast => lang.t("Add to breakfast"),
            Meal::Lunch => lang.t("Add to lunch"),
            Meal::Dinner => lang.t("Add to dinner"),
            Meal::Snacks => lang.t("Add to snacks"),
        }
    }

    pub fn add_food_title(self, lang: Lang) -> &'static str {
        match self {
            Meal::Breakfast => lang.t("Add food to breakfast"),
            Meal::Lunch => lang.t("Add food to lunch"),
            Meal::Dinner => lang.t("Add food to dinner"),
            Meal::Snacks => lang.t("Add food to snacks"),
        }
    }
}

#[derive(Clone, Debug, sqlx::FromRow, serde::Serialize, serde::Deserialize)]
pub struct Entry {
    pub id: i64,
    pub diary_date: String,
    pub meal: String,
    pub food_id: Option<i64>,
    pub amount: f64,
    pub unit: String,
    pub food_name: String,
    pub food_brand: Option<String>,
    pub source: String,
    pub energy_kcal: Option<f64>,
    pub protein_g: Option<f64>,
    pub carbs_g: Option<f64>,
    pub fat_g: Option<f64>,
    pub fiber_g: Option<f64>,
    pub sugars_g: Option<f64>,
    pub sat_fat_g: Option<f64>,
    pub salt_g: Option<f64>,
}

const ENTRY_COLUMNS: &str = "id, diary_date, meal, food_id, amount, unit, food_name, food_brand, \
    source, energy_kcal, protein_g, carbs_g, fat_g, fiber_g, sugars_g, sat_fat_g, salt_g";

impl Entry {
    pub fn meal(&self) -> Meal {
        Meal::parse(&self.meal).unwrap_or(Meal::Snacks)
    }

    /// Snapshot values per 100 units.
    pub fn per_100(&self) -> Nutrients {
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

    /// Nutrients for the amount eaten.
    pub fn nutrients(&self) -> Nutrients {
        self.per_100().scaled(self.amount)
    }
}

/// Validates an amount eaten in the food's basis unit.
pub fn check_amount(v: f64, lang: Lang) -> Result<f64, String> {
    if !v.is_finite() {
        return Err(lang.t("Enter a number.").into());
    }
    if v <= 0.0 {
        return Err(lang.t("Amount must be greater than zero.").into());
    }
    if v > MAX_AMOUNT {
        return Err(lang.f("Amount must be at most {0}.", &[&MAX_AMOUNT]));
    }
    Ok(v)
}

pub struct MealGroup {
    pub meal: Meal,
    pub entries: Vec<Entry>,
    pub totals: Totals,
}

pub struct Day {
    pub meals: Vec<MealGroup>,
    pub totals: Totals,
}

/// Groups entries by meal and computes subtotals and the day total.
pub fn summarize(entries: Vec<Entry>) -> Day {
    let mut meals: Vec<MealGroup> = Meal::ALL
        .into_iter()
        .map(|meal| MealGroup {
            meal,
            entries: Vec::new(),
            totals: Totals::default(),
        })
        .collect();
    let mut totals = Totals::default();
    for e in entries {
        let n = e.nutrients();
        let idx = Meal::ALL.iter().position(|m| *m == e.meal()).unwrap_or(3);
        meals[idx].totals.add(&n);
        totals.add(&n);
        meals[idx].entries.push(e);
    }
    Day { meals, totals }
}

/// Shows each entry's food under its official name in `lang`, if it has one.
async fn localize(db: &SqlitePool, mut entries: Vec<Entry>, lang: Lang) -> AppResult<Vec<Entry>> {
    let names = crate::foods::load_names(db, entries.iter().filter_map(|e| e.food_id)).await?;
    for e in &mut entries {
        if let Some(name) = e
            .food_id
            .and_then(|id| names.get(&id))
            .and_then(|ns| ns.iter().find(|n| n.lang == lang.code()))
        {
            e.food_name = name.name.clone();
        }
    }
    Ok(entries)
}

pub async fn entries_for_date(db: &SqlitePool, date: Date, lang: Lang) -> AppResult<Vec<Entry>> {
    let entries = sqlx::query_as::<_, Entry>(AssertSqlSafe(format!(
        "SELECT {ENTRY_COLUMNS} FROM diary_entries WHERE diary_date = ? ORDER BY created_at, id"
    )))
    .bind(date.to_string())
    .fetch_all(db)
    .await?;
    localize(db, entries, lang).await
}

pub async fn get(db: &SqlitePool, id: i64, lang: Lang) -> AppResult<Option<Entry>> {
    let entry = sqlx::query_as::<_, Entry>(AssertSqlSafe(format!(
        "SELECT {ENTRY_COLUMNS} FROM diary_entries WHERE id = ?"
    )))
    .bind(id)
    .fetch_optional(db)
    .await?;
    Ok(localize(db, entry.into_iter().collect(), lang).await?.pop())
}

/// Logs `amount` of `food`, snapshotting its current nutrition.
pub async fn create(
    db: &SqlitePool,
    date: Date,
    meal: Meal,
    food: &Food,
    amount: f64,
) -> AppResult<i64> {
    let n = food.nutrients();
    let basis: Basis = food.basis();
    let id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO diary_entries (diary_date, meal, food_id, amount, unit, food_name, food_brand,
             source, energy_kcal, protein_g, carbs_g, fat_g, fiber_g, sugars_g, sat_fat_g, salt_g)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(date.to_string())
    .bind(meal.as_str())
    .bind(food.id)
    .bind(amount)
    .bind(basis.unit())
    .bind(&food.name)
    .bind(&food.brand)
    .bind(&food.source)
    .bind(n.energy_kcal)
    .bind(n.protein_g)
    .bind(n.carbs_g)
    .bind(n.fat_g)
    .bind(n.fiber_g)
    .bind(n.sugars_g)
    .bind(n.sat_fat_g)
    .bind(n.salt_g)
    .fetch_one(db)
    .await?;
    Ok(id)
}

/// Changes amount, meal or date. The nutrition snapshot is kept.
pub async fn update(
    db: &SqlitePool,
    id: i64,
    date: Date,
    meal: Meal,
    amount: f64,
) -> AppResult<bool> {
    let res = sqlx::query(
        "UPDATE diary_entries SET diary_date = ?, meal = ?, amount = ?,
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
         WHERE id = ?",
    )
    .bind(date.to_string())
    .bind(meal.as_str())
    .bind(amount)
    .bind(id)
    .execute(db)
    .await?;
    Ok(res.rows_affected() > 0)
}

pub async fn delete(db: &SqlitePool, id: i64) -> AppResult<bool> {
    let res = sqlx::query("DELETE FROM diary_entries WHERE id = ?")
        .bind(id)
        .execute(db)
        .await?;
    Ok(res.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(meal: &str, amount: f64, kcal: Option<f64>) -> Entry {
        Entry {
            id: 1,
            diary_date: "2026-10-22".into(),
            meal: meal.into(),
            food_id: None,
            amount,
            unit: "g".into(),
            food_name: "x".into(),
            food_brand: None,
            source: "user".into(),
            energy_kcal: kcal,
            protein_g: Some(10.0),
            carbs_g: Some(20.0),
            fat_g: Some(5.0),
            fiber_g: None,
            sugars_g: None,
            sat_fat_g: None,
            salt_g: None,
        }
    }

    #[test]
    fn summarizes_meals_and_day() {
        let day = summarize(vec![
            entry("breakfast", 50.0, Some(400.0)),
            entry("breakfast", 200.0, Some(50.0)),
            entry("dinner", 150.0, Some(100.0)),
        ]);
        assert_eq!(day.meals.len(), 4);
        assert_eq!(day.meals[0].entries.len(), 2);
        assert!((day.meals[0].totals.energy_kcal - 300.0).abs() < 1e-9);
        assert!((day.meals[2].totals.energy_kcal - 150.0).abs() < 1e-9);
        assert_eq!(day.meals[1].entries.len(), 0);
        assert!((day.totals.energy_kcal - 450.0).abs() < 1e-9);
        assert!((day.totals.protein_g - 40.0).abs() < 1e-9);
        assert!(!day.totals.incomplete);
    }

    #[test]
    fn missing_energy_marks_totals_incomplete() {
        let day = summarize(vec![entry("lunch", 100.0, None)]);
        assert!(day.totals.incomplete);
        assert!(day.meals[1].totals.incomplete);
        assert!(!day.meals[0].totals.incomplete);
    }

    #[test]
    fn amount_validation() {
        assert_eq!(check_amount(150.0, Lang::En), Ok(150.0));
        assert!(check_amount(0.0, Lang::En).is_err());
        assert!(check_amount(-5.0, Lang::En).is_err());
        assert!(check_amount(f64::NAN, Lang::En).is_err());
        assert!(check_amount(20_000.0, Lang::En).is_err());
    }

    #[test]
    fn meal_round_trip() {
        for m in Meal::ALL {
            assert_eq!(Meal::parse(m.as_str()), Some(m));
        }
        assert_eq!(Meal::parse("brunch"), None);
    }
}
