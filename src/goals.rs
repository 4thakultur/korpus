//! Calorie goal estimation and goal settings persistence.
//!
//! The estimate uses the Mifflin–St Jeor equation for resting energy
//! expenditure, multiplied by a conventional activity factor. It is an
//! approximation for adults and not medical advice.

use jiff::civil::Date;
use sqlx::SqlitePool;

use crate::error::AppResult;
use crate::i18n::Lang;

/// Bump when the estimation method changes, so stored targets can be traced
/// back to the calculation that produced them.
pub const CALCULATION_VERSION: i64 = 1;

/// Never suggest an intake below this floor, whatever the inputs.
pub const MIN_SUGGESTED_KCAL: f64 = 1200.0;
/// Custom targets outside this range are rejected as implausible.
pub const MIN_CUSTOM_KCAL: f64 = 800.0;
pub const MAX_CUSTOM_KCAL: f64 = 6000.0;
/// Custom targets below this get a non-blocking caution.
pub const LOW_TARGET_CAUTION_KCAL: f64 = 1200.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sex {
    Male,
    Female,
}

impl Sex {
    pub fn as_str(self) -> &'static str {
        match self {
            Sex::Male => "male",
            Sex::Female => "female",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "male" => Some(Sex::Male),
            "female" => Some(Sex::Female),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activity {
    Sedentary,
    Light,
    Moderate,
    Active,
    VeryActive,
}

impl Activity {
    pub const ALL: [Activity; 5] = [
        Activity::Sedentary,
        Activity::Light,
        Activity::Moderate,
        Activity::Active,
        Activity::VeryActive,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Activity::Sedentary => "sedentary",
            Activity::Light => "light",
            Activity::Moderate => "moderate",
            Activity::Active => "active",
            Activity::VeryActive => "very_active",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Activity::ALL.into_iter().find(|a| a.as_str() == s)
    }

    pub fn multiplier(self) -> f64 {
        match self {
            Activity::Sedentary => 1.2,
            Activity::Light => 1.375,
            Activity::Moderate => 1.55,
            Activity::Active => 1.725,
            Activity::VeryActive => 1.9,
        }
    }

    pub fn label(self, lang: Lang) -> &'static str {
        match self {
            Activity::Sedentary => lang.t("Sedentary — little or no exercise (×1.2)"),
            Activity::Light => lang.t("Lightly active — light exercise 1–3 days/week (×1.375)"),
            Activity::Moderate => lang.t("Moderately active — exercise 3–5 days/week (×1.55)"),
            Activity::Active => lang.t("Very active — hard exercise 6–7 days/week (×1.725)"),
            Activity::VeryActive => {
                lang.t("Extra active — physical job or training twice a day (×1.9)")
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Goal {
    Maintain,
    LoseSlow,
    Lose,
    GainSlow,
}

impl Goal {
    pub const ALL: [Goal; 4] = [Goal::Maintain, Goal::LoseSlow, Goal::Lose, Goal::GainSlow];

    pub fn as_str(self) -> &'static str {
        match self {
            Goal::Maintain => "maintain",
            Goal::LoseSlow => "lose_slow",
            Goal::Lose => "lose",
            Goal::GainSlow => "gain_slow",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Goal::ALL.into_iter().find(|g| g.as_str() == s)
    }

    /// Daily adjustment relative to maintenance, deliberately conservative.
    pub fn adjustment_kcal(self) -> f64 {
        match self {
            Goal::Maintain => 0.0,
            Goal::LoseSlow => -250.0,
            Goal::Lose => -500.0,
            Goal::GainSlow => 250.0,
        }
    }

    pub fn is_loss(self) -> bool {
        matches!(self, Goal::LoseSlow | Goal::Lose)
    }

    pub fn label(self, lang: Lang) -> &'static str {
        match self {
            Goal::Maintain => lang.t("Maintain weight"),
            Goal::LoseSlow => lang.t("Lose slowly (about −250 kcal/day)"),
            Goal::Lose => lang.t("Lose (about −500 kcal/day)"),
            Goal::GainSlow => lang.t("Gain slowly (about +250 kcal/day)"),
        }
    }
}

/// Inputs for an estimate. All are required for a calculation.
#[derive(Clone, Copy, Debug)]
pub struct EstimateInput {
    pub sex: Sex,
    pub age_years: i32,
    pub height_cm: f64,
    pub weight_kg: f64,
    pub activity: Activity,
    pub goal: Goal,
    /// Pregnancy, breastfeeding or a medical condition affecting nutrition.
    pub special_circumstances: bool,
}

/// Explains why a suggested target differs from the plain calculation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EstimateNote {
    /// A loss goal was replaced by maintenance.
    SpecialCircumstances,
    /// The target was raised to the safety floor.
    LimitedToFloor,
}

impl EstimateNote {
    pub fn message(self, lang: Lang) -> &'static str {
        match self {
            EstimateNote::SpecialCircumstances => lang.t(
                "Because you indicated pregnancy, breastfeeding or a medical condition, Korpus does not suggest a weight-loss target. Please ask a qualified health professional for guidance.",
            ),
            EstimateNote::LimitedToFloor => lang.t(
                "The suggested target was limited so it does not fall below your estimated resting energy needs or 1200 kcal.",
            ),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Estimate {
    /// The equation is validated for adults only; no estimate is offered.
    NotForMinors,
    Calculated {
        bmr_kcal: f64,
        maintenance_kcal: f64,
        /// Suggested daily target after the goal adjustment and safety floor.
        target_kcal: f64,
        /// The goal that was actually applied (a loss goal may be replaced by
        /// maintenance, see `notes`).
        applied_goal: Goal,
        notes: Vec<EstimateNote>,
    },
}

/// Mifflin–St Jeor resting energy expenditure in kcal/day.
pub fn mifflin_st_jeor(sex: Sex, age_years: i32, height_cm: f64, weight_kg: f64) -> f64 {
    let s = match sex {
        Sex::Male => 5.0,
        Sex::Female => -161.0,
    };
    10.0 * weight_kg + 6.25 * height_cm - 5.0 * f64::from(age_years) + s
}

pub fn estimate(input: &EstimateInput) -> Estimate {
    if input.age_years < 18 {
        return Estimate::NotForMinors;
    }
    let bmr = mifflin_st_jeor(input.sex, input.age_years, input.height_cm, input.weight_kg);
    let maintenance = bmr * input.activity.multiplier();
    let mut notes = Vec::new();

    let mut goal = input.goal;
    if goal.is_loss() && input.special_circumstances {
        goal = Goal::Maintain;
        notes.push(EstimateNote::SpecialCircumstances);
    }

    let mut target = maintenance + goal.adjustment_kcal();
    let floor = MIN_SUGGESTED_KCAL.max(bmr);
    if goal.is_loss() && target < floor {
        target = floor.min(maintenance);
        notes.push(EstimateNote::LimitedToFloor);
    }

    Estimate::Calculated {
        bmr_kcal: bmr,
        maintenance_kcal: maintenance,
        target_kcal: target,
        applied_goal: goal,
        notes,
    }
}

/// Whole years between `birth` and `on`.
pub fn age_on(birth: Date, on: Date) -> i32 {
    let mut age = i32::from(on.year()) - i32::from(birth.year());
    if (on.month(), on.day()) < (birth.month(), birth.day()) {
        age -= 1;
    }
    age
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CustomTargetCheck {
    Ok,
    Low,
}

pub fn check_custom_target(kcal: f64, lang: Lang) -> Result<CustomTargetCheck, String> {
    if !kcal.is_finite() {
        return Err(lang.t("Enter a number.").into());
    }
    if !(MIN_CUSTOM_KCAL..=MAX_CUSTOM_KCAL).contains(&kcal) {
        return Err(lang.f(
            "Enter a target between {0} and {1} kcal.",
            &[&MIN_CUSTOM_KCAL, &MAX_CUSTOM_KCAL],
        ));
    }
    if kcal < LOW_TARGET_CAUTION_KCAL {
        Ok(CustomTargetCheck::Low)
    } else {
        Ok(CustomTargetCheck::Ok)
    }
}

#[derive(Clone, Debug, Default, sqlx::FromRow, serde::Serialize, serde::Deserialize)]
pub struct GoalSettings {
    pub daily_kcal_target: Option<f64>,
    pub target_source: Option<String>,
    pub goal: String,
    pub sex: Option<String>,
    pub birth_date: Option<String>,
    pub height_cm: Option<f64>,
    pub weight_kg: Option<f64>,
    pub activity: Option<String>,
    pub special_circumstances: bool,
    pub maintenance_kcal: Option<f64>,
    pub calculation_version: Option<i64>,
    pub protein_goal_g: Option<f64>,
    pub carbs_goal_g: Option<f64>,
    pub fat_goal_g: Option<f64>,
    pub updated_at: String,
}

pub async fn load(db: &SqlitePool) -> AppResult<GoalSettings> {
    Ok(sqlx::query_as::<_, GoalSettings>(
        "SELECT daily_kcal_target, target_source, goal, sex, birth_date, height_cm, weight_kg,
                activity, special_circumstances, maintenance_kcal, calculation_version,
                protein_goal_g, carbs_goal_g, fat_goal_g, updated_at
         FROM goal_settings WHERE id = 1",
    )
    .fetch_one(db)
    .await?)
}

pub async fn save(db: &SqlitePool, g: &GoalSettings) -> AppResult<()> {
    sqlx::query(
        "UPDATE goal_settings SET daily_kcal_target = ?, target_source = ?, goal = ?, sex = ?,
                birth_date = ?, height_cm = ?, weight_kg = ?, activity = ?,
                special_circumstances = ?, maintenance_kcal = ?, calculation_version = ?,
                protein_goal_g = ?, carbs_goal_g = ?, fat_goal_g = ?,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
         WHERE id = 1",
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
    .execute(db)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    fn input() -> EstimateInput {
        EstimateInput {
            sex: Sex::Male,
            age_years: 30,
            height_cm: 180.0,
            weight_kg: 80.0,
            activity: Activity::Sedentary,
            goal: Goal::Maintain,
            special_circumstances: false,
        }
    }

    #[test]
    fn mifflin_st_jeor_reference_values() {
        // 10*80 + 6.25*180 - 5*30 + 5 = 1780
        assert_eq!(mifflin_st_jeor(Sex::Male, 30, 180.0, 80.0), 1780.0);
        // 10*60 + 6.25*165 - 5*40 - 161 = 1270.25
        assert_eq!(mifflin_st_jeor(Sex::Female, 40, 165.0, 60.0), 1270.25);
    }

    #[test]
    fn maintenance_uses_activity_multiplier() {
        let mut i = input();
        i.activity = Activity::Moderate;
        let Estimate::Calculated {
            maintenance_kcal,
            target_kcal,
            ..
        } = estimate(&i)
        else {
            panic!("expected estimate");
        };
        assert!((maintenance_kcal - 1780.0 * 1.55).abs() < 1e-9);
        assert_eq!(target_kcal, maintenance_kcal);
    }

    #[test]
    fn loss_goal_applies_conservative_deficit() {
        let mut i = input();
        i.activity = Activity::Moderate;
        i.goal = Goal::Lose;
        let Estimate::Calculated {
            maintenance_kcal,
            target_kcal,
            ..
        } = estimate(&i)
        else {
            panic!("expected estimate");
        };
        assert!((target_kcal - (maintenance_kcal - 500.0)).abs() < 1e-9);
    }

    #[test]
    fn loss_target_never_below_bmr_or_floor() {
        let i = EstimateInput {
            sex: Sex::Female,
            age_years: 70,
            height_cm: 150.0,
            weight_kg: 45.0,
            activity: Activity::Sedentary,
            goal: Goal::Lose,
            special_circumstances: false,
        };
        let Estimate::Calculated {
            bmr_kcal,
            maintenance_kcal,
            target_kcal,
            notes,
            ..
        } = estimate(&i)
        else {
            panic!("expected estimate");
        };
        assert!(target_kcal >= bmr_kcal.min(maintenance_kcal));
        assert!(target_kcal <= maintenance_kcal);
        assert!(target_kcal >= MIN_SUGGESTED_KCAL.min(maintenance_kcal));
        assert!(!notes.is_empty());
    }

    #[test]
    fn no_estimate_for_minors() {
        let mut i = input();
        i.age_years = 17;
        assert_eq!(estimate(&i), Estimate::NotForMinors);
    }

    #[test]
    fn special_circumstances_block_loss_target() {
        let mut i = input();
        i.goal = Goal::Lose;
        i.special_circumstances = true;
        let Estimate::Calculated {
            applied_goal,
            target_kcal,
            maintenance_kcal,
            ..
        } = estimate(&i)
        else {
            panic!("expected estimate");
        };
        assert_eq!(applied_goal, Goal::Maintain);
        assert_eq!(target_kcal, maintenance_kcal);
    }

    #[test]
    fn age_respects_birthday() {
        assert_eq!(age_on(date(1990, 6, 15), date(2026, 6, 14)), 35);
        assert_eq!(age_on(date(1990, 6, 15), date(2026, 6, 15)), 36);
        assert_eq!(age_on(date(2000, 2, 29), date(2026, 2, 28)), 25);
        assert_eq!(age_on(date(2000, 2, 29), date(2026, 3, 1)), 26);
    }

    #[test]
    fn custom_target_bounds() {
        assert_eq!(
            check_custom_target(2000.0, Lang::En),
            Ok(CustomTargetCheck::Ok)
        );
        assert_eq!(
            check_custom_target(1000.0, Lang::En),
            Ok(CustomTargetCheck::Low)
        );
        assert!(check_custom_target(500.0, Lang::En).is_err());
        assert!(check_custom_target(9000.0, Lang::En).is_err());
        assert!(check_custom_target(f64::NAN, Lang::En).is_err());
    }
}
