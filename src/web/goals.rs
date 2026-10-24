//! Goal setup: optional estimate inputs, the daily target and macro goals.
//!
//! The target is only recalculated when this form is submitted; logging a
//! new weight never changes it silently.

use askama::Template;
use axum::Form;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use super::{AppState, current_lang, flash_message, render, render_status, see_other};
use crate::error::AppResult;
use crate::forms::{FieldErrors, InputField};
use crate::goals::{
    self, Activity, CALCULATION_VERSION, CustomTargetCheck, Estimate, EstimateInput, Goal,
    GoalSettings, MacroEstimate, MacroInput, Sex, age_on, check_custom_target, estimate,
    estimate_macros,
};
use crate::i18n::Lang;
use crate::nutrition::{Nutrient, fmt_g_whole, fmt_kcal};
use crate::units::{UnitSystem, parse_date, parse_decimal, today_in};
use crate::{profile, weight};

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct GoalsForm {
    pub target_mode: String,
    pub sex: String,
    pub birth_date: String,
    pub height: String,
    pub weight: String,
    pub activity: String,
    pub goal: String,
    pub special: String,
    pub custom_target: String,
    pub macro_mode: String,
    pub protein_goal: String,
    pub carbs_goal: String,
    pub fat_goal: String,
}

pub struct EstimateView {
    pub bmr: String,
    pub maintenance: String,
    pub target: String,
    pub applied_goal: &'static str,
    pub notes: Vec<&'static str>,
}

/// The macronutrient goals that the estimate would give.
pub struct MacroPlanView {
    pub protein_label: &'static str,
    pub carbs_label: &'static str,
    pub fat_label: &'static str,
    pub protein: String,
    pub carbs: String,
    pub fat: String,
    /// "1.6 g per kg of body weight"
    pub protein_basis: String,
    pub protein_limited: bool,
}

pub struct Choice {
    pub value: &'static str,
    pub label: &'static str,
}

#[derive(Template)]
#[template(path = "goals.html")]
pub struct GoalsPage {
    pub lang: Lang,
    pub title: String,
    pub nav: &'static str,
    pub form: GoalsForm,
    pub errors: FieldErrors,
    pub message: Option<&'static str>,
    pub current_target: Option<String>,
    pub current_source: &'static str,
    pub estimate: Option<EstimateView>,
    pub macro_plan: Option<MacroPlanView>,
    pub minor: bool,
    pub caution: Option<&'static str>,
    pub weight_unit: &'static str,
    pub height_unit: &'static str,
    pub activities: Vec<Choice>,
    pub goals: Vec<Choice>,
    pub setup: bool,
}

impl GoalsPage {
    pub fn macro_fields(&self) -> Vec<InputField> {
        let f = &self.form;
        let lang = self.lang;
        let field = |id, label, value: &String| InputField {
            id,
            label,
            value: value.clone(),
            required: false,
        };
        vec![
            field(
                "protein_goal",
                Nutrient::Protein.label(lang),
                &f.protein_goal,
            ),
            field("carbs_goal", Nutrient::Carbs.label(lang), &f.carbs_goal),
            field("fat_goal", Nutrient::Fat.label(lang), &f.fat_goal),
        ]
    }
}

fn fmt_input(v: Option<f64>, decimals: usize) -> String {
    v.map(|v| format!("{v:.decimals$}")).unwrap_or_default()
}

fn form_from_settings(g: &GoalSettings, units: UnitSystem, latest_kg: Option<f64>) -> GoalsForm {
    GoalsForm {
        target_mode: g.target_source.clone().unwrap_or_else(|| "none".into()),
        sex: g.sex.clone().unwrap_or_default(),
        birth_date: g.birth_date.clone().unwrap_or_default(),
        height: fmt_input(g.height_cm.map(|c| units.height_from_cm(c)), 1),
        weight: fmt_input(
            g.weight_kg.or(latest_kg).map(|k| units.weight_from_kg(k)),
            1,
        ),
        activity: g.activity.clone().unwrap_or_default(),
        goal: g.goal.clone(),
        special: if g.special_circumstances {
            "on".into()
        } else {
            String::new()
        },
        custom_target: if g.target_source.as_deref() == Some("custom") {
            fmt_input(g.daily_kcal_target, 0)
        } else {
            String::new()
        },
        macro_mode: g.macro_source.clone().unwrap_or_else(|| "custom".into()),
        protein_goal: fmt_input(g.protein_goal_g, 0),
        carbs_goal: fmt_input(g.carbs_goal_g, 0),
        fat_goal: fmt_input(g.fat_goal_g, 0),
    }
}

fn estimate_view(e: &Estimate, lang: Lang) -> Option<EstimateView> {
    match e {
        Estimate::NotForMinors => None,
        Estimate::Calculated {
            bmr_kcal,
            maintenance_kcal,
            target_kcal,
            applied_goal,
            notes,
        } => Some(EstimateView {
            bmr: fmt_kcal(*bmr_kcal),
            maintenance: fmt_kcal(*maintenance_kcal),
            target: fmt_kcal(*target_kcal),
            applied_goal: applied_goal.label(lang),
            notes: notes.iter().map(|n| n.message(lang)).collect(),
        }),
    }
}

/// Parsed estimate inputs. Each is optional; an estimate needs all of them.
struct Parsed {
    sex: Option<Sex>,
    birth_date: Option<jiff::civil::Date>,
    height_cm: Option<f64>,
    weight_kg: Option<f64>,
    activity: Option<Activity>,
    goal: Goal,
    special: bool,
}

fn parse_inputs(
    form: &GoalsForm,
    units: UnitSystem,
    errors: &mut FieldErrors,
    today: jiff::civil::Date,
    lang: Lang,
) -> Parsed {
    let sex = match form.sex.as_str() {
        "" => None,
        s => Sex::parse(s).or_else(|| {
            errors.add("sex", lang.t("Choose an option."));
            None
        }),
    };
    let birth_date = match form.birth_date.trim() {
        "" => None,
        s => match parse_date(s) {
            Some(d) if d <= today && age_on(d, today) <= 120 => Some(d),
            _ => {
                errors.add("birth_date", lang.t("Enter a valid date of birth."));
                None
            }
        },
    };
    let mut number = |field: &'static str,
                      raw: &str,
                      to_canonical: &dyn Fn(f64) -> f64,
                      range: (f64, f64),
                      msg: &str| {
        match parse_decimal(raw) {
            Ok(None) => None,
            Ok(Some(v)) => {
                let c = to_canonical(v);
                if (range.0..=range.1).contains(&c) {
                    Some(c)
                } else {
                    errors.add(field, msg.to_string());
                    None
                }
            }
            Err(e) => {
                errors.add(field, e.for_field(lang, lang.t("This value")));
                None
            }
        }
    };
    let height_cm = number(
        "height",
        &form.height,
        &|v| units.height_to_cm(v),
        (100.0, 250.0),
        lang.t("Enter a height between 100 and 250 cm (39–98 in)."),
    );
    let weight_kg = number(
        "weight",
        &form.weight,
        &|v| units.weight_to_kg(v),
        (weight::MIN_KG, weight::MAX_KG),
        lang.t("Enter a weight between 20 and 400 kg (44–880 lb)."),
    );
    let activity = match form.activity.as_str() {
        "" => None,
        s => Activity::parse(s).or_else(|| {
            errors.add("activity", lang.t("Choose an activity level."));
            None
        }),
    };
    let goal = Goal::parse(&form.goal).unwrap_or(Goal::Maintain);
    Parsed {
        sex,
        birth_date,
        height_cm,
        weight_kg,
        activity,
        goal,
        special: !form.special.is_empty(),
    }
}

fn macro_goal(raw: &str, field: &'static str, errors: &mut FieldErrors, lang: Lang) -> Option<f64> {
    match parse_decimal(raw) {
        Ok(None) => None,
        Ok(Some(v)) if (0.0..=1000.0).contains(&v) => Some(v),
        Ok(Some(_)) => {
            errors.add(field, lang.t("Enter a value between 0 and 1000 g."));
            None
        }
        Err(e) => {
            errors.add(field, e.for_field(lang, lang.t("This value")));
            None
        }
    }
}

/// The goal the estimates apply: a loss goal becomes maintenance when the
/// user indicated special circumstances, as in [`goals::estimate`].
fn applied_goal(p: &Parsed) -> Goal {
    if p.special && p.goal.is_loss() {
        Goal::Maintain
    } else {
        p.goal
    }
}

fn is_minor(p: &Parsed, today: jiff::civil::Date) -> bool {
    p.birth_date.is_some_and(|d| age_on(d, today) < 18)
}

/// The calorie target the form would save, if it is valid.
fn planned_target(form: &GoalsForm, est: Option<&Estimate>, lang: Lang) -> Option<f64> {
    match form.target_mode.as_str() {
        "estimate" => match est? {
            Estimate::Calculated { target_kcal, .. } => Some(*target_kcal),
            Estimate::NotForMinors => None,
        },
        "custom" => parse_decimal(&form.custom_target)
            .ok()
            .flatten()
            .filter(|v| check_custom_target(*v, lang).is_ok()),
        _ => None,
    }
}

/// Macro goals from the target, `None` when the target or weight is missing
/// or the user is under 18.
fn macro_estimate_for(
    p: &Parsed,
    target_kcal: Option<f64>,
    today: jiff::civil::Date,
) -> Option<MacroEstimate> {
    if is_minor(p, today) {
        return None;
    }
    Some(estimate_macros(&MacroInput {
        target_kcal: target_kcal?,
        weight_kg: p.weight_kg?,
        goal: applied_goal(p),
        activity: p.activity,
    }))
}

fn macro_plan_view(m: &MacroEstimate, units: UnitSystem, lang: Lang) -> MacroPlanView {
    // Per lb for imperial units, so the rate matches the weight shown.
    let protein_basis = match units {
        UnitSystem::Metric => lang.f1(
            "{0} g per kg of body weight",
            lang.num(format!("{:.1}", m.protein_g_per_kg)),
        ),
        UnitSystem::Imperial => lang.f1(
            "{0} g per lb of body weight",
            lang.num(format!(
                "{:.2}",
                m.protein_g_per_kg / units.weight_from_kg(1.0)
            )),
        ),
    };
    MacroPlanView {
        protein_label: Nutrient::Protein.label(lang),
        carbs_label: Nutrient::Carbs.label(lang),
        fat_label: Nutrient::Fat.label(lang),
        protein: fmt_g_whole(m.protein_g),
        carbs: fmt_g_whole(m.carbs_g),
        fat: fmt_g_whole(m.fat_g),
        protein_basis,
        protein_limited: m.protein_limited,
    }
}

fn current_source(g: &GoalSettings, lang: Lang) -> &'static str {
    match g.target_source.as_deref() {
        Some("estimate") => lang.t("estimated"),
        Some("custom") => lang.t("set by you"),
        _ => "",
    }
}

fn estimate_for(p: &Parsed, today: jiff::civil::Date) -> Option<Estimate> {
    Some(estimate(&EstimateInput {
        sex: p.sex?,
        age_years: age_on(p.birth_date?, today),
        height_cm: p.height_cm?,
        weight_kg: p.weight_kg?,
        activity: p.activity?,
        goal: p.goal,
        special_circumstances: p.special,
    }))
}

fn choices(lang: Lang) -> (Vec<Choice>, Vec<Choice>) {
    (
        Activity::ALL
            .into_iter()
            .map(|a| Choice {
                value: a.as_str(),
                label: a.label(lang),
            })
            .collect(),
        Goal::ALL
            .into_iter()
            .map(|g| Choice {
                value: g.as_str(),
                label: g.label(lang),
            })
            .collect(),
    )
}

#[derive(Deserialize)]
pub struct GoalsQuery {
    msg: Option<String>,
    setup: Option<String>,
}

pub async fn page(
    State(state): State<AppState>,
    Query(q): Query<GoalsQuery>,
) -> AppResult<Response> {
    let lang = current_lang();
    let p = profile::load(&state.db).await?;
    let units = p.units();
    let today = today_in(&p.timezone);
    let g = goals::load(&state.db).await?;
    let latest = weight::latest(&state.db).await?.map(|w| w.weight_kg);
    let form = form_from_settings(&g, units, latest);

    let mut scratch = FieldErrors::default();
    let parsed = parse_inputs(&form, units, &mut scratch, today, lang);
    let est = estimate_for(&parsed, today);
    let macros = macro_estimate_for(&parsed, planned_target(&form, est.as_ref(), lang), today);
    let (activities, goal_choices) = choices(lang);
    let page = GoalsPage {
        lang,
        title: lang.t("Goals").into(),
        nav: "goals",
        errors: FieldErrors::default(),
        message: flash_message(q.msg.as_deref(), lang),
        current_target: g.daily_kcal_target.map(fmt_kcal),
        current_source: current_source(&g, lang),
        estimate: est.as_ref().and_then(|e| estimate_view(e, lang)),
        macro_plan: macros.map(|m| macro_plan_view(&m, units, lang)),
        minor: matches!(est, Some(Estimate::NotForMinors)),
        caution: None,
        weight_unit: units.weight_unit(),
        height_unit: units.height_unit(),
        activities,
        goals: goal_choices,
        setup: q.setup.is_some(),
        form,
    };
    Ok(render(&page)?.into_response())
}

pub async fn save(
    State(state): State<AppState>,
    Form(form): Form<GoalsForm>,
) -> AppResult<Response> {
    let lang = current_lang();
    let p = profile::load(&state.db).await?;
    let units = p.units();
    let today = today_in(&p.timezone);
    let mut errors = FieldErrors::default();
    let parsed = parse_inputs(&form, units, &mut errors, today, lang);
    let est = estimate_for(&parsed, today);

    let mut caution = None;
    let (target, source, maintenance) = match form.target_mode.as_str() {
        "estimate" => match &est {
            Some(Estimate::Calculated {
                target_kcal,
                maintenance_kcal,
                ..
            }) => (
                Some(*target_kcal),
                Some("estimate"),
                Some(*maintenance_kcal),
            ),
            Some(Estimate::NotForMinors) => {
                errors.add(
                    "target_mode",
                    lang.t("Korpus does not estimate calorie targets for people under 18. Please ask a doctor or registered dietitian, or choose no target."),
                );
                (None, None, None)
            }
            None => {
                errors.add(
                    "target_mode",
                    lang.t("To estimate a target, fill in sex, date of birth, height, weight and activity level."),
                );
                (None, None, None)
            }
        },
        "custom" => match parse_decimal(&form.custom_target) {
            Ok(Some(v)) => match check_custom_target(v, lang) {
                Ok(check) => {
                    if check == CustomTargetCheck::Low {
                        caution = Some(lang.t(
                            "This target is below 1200 kcal. Very low intakes are not suitable for most adults without medical supervision.",
                        ));
                    }
                    (Some(v), Some("custom"), None)
                }
                Err(e) => {
                    errors.add("custom_target", e);
                    (None, None, None)
                }
            },
            Ok(None) => {
                errors.add("custom_target", lang.t("Enter your daily target in kcal."));
                (None, None, None)
            }
            Err(e) => {
                errors.add("custom_target", e.for_field(lang, lang.t("Target")));
                (None, None, None)
            }
        },
        "none" | "" => (None, None, None),
        _ => {
            errors.add("target_mode", lang.t("Choose how to set your target."));
            (None, None, None)
        }
    };

    let macro_est = macro_estimate_for(&parsed, target, today);
    let (protein, carbs, fat, macro_source) = match form.macro_mode.as_str() {
        "estimate" => match macro_est {
            Some(m) => (
                Some(m.protein_g),
                Some(m.carbs_g),
                Some(m.fat_g),
                Some("estimate"),
            ),
            None => {
                if is_minor(&parsed, today) {
                    errors.add(
                        "macro_mode",
                        lang.t("Korpus does not estimate macronutrient goals for people under 18. Please ask a doctor or registered dietitian, or enter your own."),
                    );
                } else if target.is_none() {
                    // A target error is already reported; don't repeat it.
                    if !errors.has("target_mode") && !errors.has("custom_target") {
                        errors.add(
                            "macro_mode",
                            lang.t("To estimate macronutrient goals, set a daily calorie target."),
                        );
                    }
                } else if !errors.has("weight") {
                    errors.add(
                        "macro_mode",
                        lang.t("To estimate macronutrient goals, enter your weight."),
                    );
                }
                (None, None, None, None)
            }
        },
        "custom" | "" => (
            macro_goal(&form.protein_goal, "protein_goal", &mut errors, lang),
            macro_goal(&form.carbs_goal, "carbs_goal", &mut errors, lang),
            macro_goal(&form.fat_goal, "fat_goal", &mut errors, lang),
            Some("custom"),
        ),
        _ => {
            errors.add("macro_mode", lang.t("Choose how to set your target."));
            (None, None, None, None)
        }
    };

    if errors.is_empty() {
        let settings = GoalSettings {
            daily_kcal_target: target,
            target_source: source.map(str::to_string),
            goal: parsed.goal.as_str().to_string(),
            sex: parsed.sex.map(|s| s.as_str().to_string()),
            birth_date: parsed.birth_date.map(|d| d.to_string()),
            height_cm: parsed.height_cm,
            weight_kg: parsed.weight_kg,
            activity: parsed.activity.map(|a| a.as_str().to_string()),
            special_circumstances: parsed.special,
            maintenance_kcal: maintenance,
            calculation_version: maintenance.map(|_| CALCULATION_VERSION),
            protein_goal_g: protein,
            carbs_goal_g: carbs,
            fat_goal_g: fat,
            macro_source: macro_source.map(str::to_string),
            updated_at: String::new(),
        };
        goals::save(&state.db, &settings).await?;
        if caution.is_none() {
            return Ok(see_other("/goals?msg=goals_saved"));
        }
    }

    let g = goals::load(&state.db).await?;
    let (activities, goal_choices) = choices(lang);
    let status = if errors.is_empty() {
        StatusCode::OK
    } else {
        StatusCode::UNPROCESSABLE_ENTITY
    };
    let page = GoalsPage {
        lang,
        title: lang.t("Goals").into(),
        nav: "goals",
        message: errors.is_empty().then_some(lang.t("Goal settings saved.")),
        errors,
        current_target: g.daily_kcal_target.map(fmt_kcal),
        current_source: current_source(&g, lang),
        estimate: est.as_ref().and_then(|e| estimate_view(e, lang)),
        macro_plan: macro_est.map(|m| macro_plan_view(&m, units, lang)),
        minor: matches!(est, Some(Estimate::NotForMinors)),
        caution,
        weight_unit: units.weight_unit(),
        height_unit: units.height_unit(),
        activities,
        goals: goal_choices,
        setup: false,
        form,
    };
    render_status(status, &page)
}
