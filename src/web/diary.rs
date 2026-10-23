//! Daily diary view and entry mutations.

use askama::Template;
use axum::Form;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use jiff::ToSpan;
use jiff::civil::Date;
use serde::Deserialize;

use super::{
    AppState, LogContext, current_lang, flash_message, parse_id, render, render_status, see_other,
};
use crate::diary::{self, Entry, Meal, check_amount};
use crate::error::{AppError, AppResult};
use crate::forms::FieldErrors;
use crate::i18n::Lang;
use crate::nutrition::{Nutrient, fmt_amount, fmt_g, fmt_kcal, fmt_opt_g, fmt_opt_kcal};
use crate::units::{fmt_date_long, parse_date, parse_decimal, today_in};
use crate::{foods, goals, profile};

pub async fn root(State(state): State<AppState>) -> AppResult<Response> {
    let p = profile::load(&state.db).await?;
    Ok(see_other(if p.setup_complete {
        "/diary"
    } else {
        "/setup"
    }))
}

#[derive(Deserialize)]
pub struct DayQuery {
    date: Option<String>,
    msg: Option<String>,
}

pub struct EntryView {
    pub id: i64,
    pub name: String,
    pub brand: String,
    pub amount: String,
    pub kcal: String,
    pub protein: String,
    pub carbs: String,
    pub fat: String,
    pub missing: bool,
}

pub struct MealView {
    pub key: &'static str,
    pub label: &'static str,
    pub kcal: String,
    pub protein: String,
    pub carbs: String,
    pub fat: String,
    pub incomplete: bool,
    pub entries: Vec<EntryView>,
    pub add_url: String,
    pub add_label: &'static str,
}

pub struct MacroView {
    pub key: &'static str,
    pub label: &'static str,
    pub consumed: String,
    pub goal: Option<String>,
    /// 0–100 for the progress element.
    pub pct: u32,
}

#[derive(Template)]
#[template(path = "diary.html")]
pub struct DayPage {
    pub lang: Lang,
    pub title: String,
    pub nav: &'static str,
    pub date_iso: String,
    pub date_label: String,
    pub prev_iso: String,
    pub next_iso: String,
    pub today_iso: String,
    pub is_today: bool,
    pub consumed: String,
    pub target: Option<String>,
    pub remaining: Option<String>,
    pub over: Option<String>,
    pub pct: u32,
    pub incomplete: bool,
    pub macros: Vec<MacroView>,
    pub meals: Vec<MealView>,
    pub message: Option<&'static str>,
    pub add_url: String,
}

fn pct(value: f64, goal: f64) -> u32 {
    if goal <= 0.0 {
        return 0;
    }
    ((value / goal) * 100.0).clamp(0.0, 100.0).round() as u32
}

fn entry_view(e: &Entry, lang: Lang) -> EntryView {
    let n = e.nutrients();
    EntryView {
        id: e.id,
        name: e.food_name.clone(),
        brand: e.food_brand.clone().unwrap_or_default(),
        amount: format!("{} {}", lang.num(fmt_amount(e.amount)), e.unit),
        kcal: fmt_opt_kcal(n.energy_kcal),
        protein: lang.num(fmt_opt_g(n.protein_g)),
        carbs: lang.num(fmt_opt_g(n.carbs_g)),
        fat: lang.num(fmt_opt_g(n.fat_g)),
        missing: !n.core_complete(),
    }
}

pub async fn day(State(state): State<AppState>, Query(q): Query<DayQuery>) -> AppResult<Response> {
    let lang = current_lang();
    let p = profile::load(&state.db).await?;
    if !p.setup_complete {
        return Ok(see_other("/setup"));
    }
    let today = today_in(&p.timezone);
    let date = match q.date.as_deref() {
        None | Some("") => today,
        Some(s) => parse_date(s).ok_or_else(|| {
            AppError::BadRequest(
                lang.t("The date must be a valid calendar date (YYYY-MM-DD).")
                    .into(),
            )
        })?,
    };
    let entries = diary::entries_for_date(&state.db, date, lang).await?;
    let summary = diary::summarize(entries);
    let g = goals::load(&state.db).await?;
    let t = summary.totals;

    let target = g.daily_kcal_target;
    let (remaining, over) = match target {
        Some(target) if t.energy_kcal <= target => (Some(fmt_kcal(target - t.energy_kcal)), None),
        Some(target) => (None, Some(fmt_kcal(t.energy_kcal - target))),
        None => (None, None),
    };

    let macro_view = |n: Nutrient, consumed: f64, goal: Option<f64>| MacroView {
        key: n.key(),
        label: n.label(lang),
        consumed: lang.num(fmt_g(consumed)),
        goal: goal.map(|g| lang.num(fmt_g(g))),
        pct: goal.map(|g| pct(consumed, g)).unwrap_or(0),
    };
    let macros = vec![
        macro_view(Nutrient::Protein, t.protein_g, g.protein_goal_g),
        macro_view(Nutrient::Carbs, t.carbs_g, g.carbs_goal_g),
        macro_view(Nutrient::Fat, t.fat_g, g.fat_goal_g),
    ];

    let meals = summary
        .meals
        .iter()
        .map(|m| MealView {
            key: m.meal.as_str(),
            label: m.meal.label(lang),
            kcal: fmt_kcal(m.totals.energy_kcal),
            protein: lang.num(fmt_g(m.totals.protein_g)),
            carbs: lang.num(fmt_g(m.totals.carbs_g)),
            fat: lang.num(fmt_g(m.totals.fat_g)),
            incomplete: m.totals.incomplete,
            entries: m.entries.iter().map(|e| entry_view(e, lang)).collect(),
            add_label: m.meal.add_label(lang),
            add_url: LogContext {
                date: Some(date),
                meal: Some(m.meal),
            }
            .url("/search"),
        })
        .collect();

    let page = DayPage {
        lang,
        title: if date == today {
            lang.t("Today").into()
        } else {
            fmt_date_long(date, lang)
        },
        nav: "diary",
        date_iso: date.to_string(),
        date_label: fmt_date_long(date, lang),
        prev_iso: shift(date, -1).to_string(),
        next_iso: shift(date, 1).to_string(),
        today_iso: today.to_string(),
        is_today: date == today,
        consumed: fmt_kcal(t.energy_kcal),
        target: target.map(fmt_kcal),
        remaining,
        over,
        pct: target.map(|g| pct(t.energy_kcal, g)).unwrap_or(0),
        incomplete: t.incomplete,
        macros,
        meals,
        message: flash_message(q.msg.as_deref(), lang),
        add_url: LogContext {
            date: Some(date),
            meal: None,
        }
        .url("/search"),
    };
    Ok(render(&page)?.into_response())
}

fn shift(d: Date, days: i32) -> Date {
    d.checked_add(days.days()).unwrap_or(d)
}

#[derive(Deserialize)]
pub struct EntryForm {
    #[serde(default)]
    food_id: String,
    #[serde(default)]
    date: String,
    #[serde(default)]
    meal: String,
    #[serde(default)]
    amount: String,
}

struct ValidEntry {
    date: Date,
    meal: Meal,
    amount: f64,
}

fn validate_entry(form: &EntryForm, lang: Lang) -> Result<ValidEntry, FieldErrors> {
    let mut errors = FieldErrors::default();
    let date = parse_date(&form.date);
    if date.is_none() {
        errors.add("date", lang.t("Choose a valid date."));
    }
    let meal = Meal::parse(&form.meal);
    if meal.is_none() {
        errors.add("meal", lang.t("Choose a meal."));
    }
    let amount = match parse_decimal(&form.amount) {
        Ok(Some(v)) => match check_amount(v, lang) {
            Ok(v) => Some(v),
            Err(e) => {
                errors.add("amount", e);
                None
            }
        },
        Ok(None) => {
            errors.add("amount", lang.t("Enter the amount you ate."));
            None
        }
        Err(e) => {
            errors.add("amount", e.for_field(lang, lang.t("Amount")));
            None
        }
    };
    match (date, meal, amount) {
        (Some(date), Some(meal), Some(amount)) => Ok(ValidEntry { date, meal, amount }),
        _ => Err(errors),
    }
}

fn diary_url(date: Date, msg: &str, meal: Meal) -> String {
    format!("/diary?date={date}&msg={msg}#meal-{}", meal.as_str())
}

pub async fn create(
    State(state): State<AppState>,
    Form(form): Form<EntryForm>,
) -> AppResult<Response> {
    let id = parse_id(&form.food_id)?;
    let food = foods::get(&state.db, id, current_lang())
        .await?
        .ok_or(AppError::NotFound)?;
    match validate_entry(&form, current_lang()) {
        Ok(v) => {
            diary::create(&state.db, v.date, v.meal, &food, v.amount).await?;
            Ok(see_other(&diary_url(v.date, "added", v.meal)))
        }
        Err(errors) => {
            let page = super::foods::detail_page(
                &state,
                food,
                &form.date,
                &form.meal,
                &form.amount,
                errors,
                None,
            )
            .await?;
            render_status(StatusCode::UNPROCESSABLE_ENTITY, &page)
        }
    }
}

#[derive(Template)]
#[template(path = "entry_edit.html")]
pub struct EntryEditPage {
    pub lang: Lang,
    pub title: String,
    pub nav: &'static str,
    pub id: i64,
    pub name: String,
    pub brand: String,
    pub source_label: &'static str,
    pub food_url: Option<String>,
    pub unit: String,
    pub amount: String,
    pub date: String,
    pub meal: String,
    pub meals: [Meal; 4],
    pub per100_kcal: String,
    pub per100_protein: String,
    pub per100_carbs: String,
    pub per100_fat: String,
    pub data_kcal: String,
    pub data_protein: String,
    pub data_carbs: String,
    pub data_fat: String,
    pub errors: FieldErrors,
    pub back_url: String,
}

fn data_attr(v: Option<f64>) -> String {
    v.map(|v| v.to_string()).unwrap_or_default()
}

fn edit_page(
    e: &Entry,
    amount: &str,
    date: &str,
    meal: &str,
    errors: FieldErrors,
    lang: Lang,
) -> EntryEditPage {
    let per = e.per_100();
    EntryEditPage {
        lang,
        title: lang.f("Edit {0}", &[&e.food_name]),
        nav: "diary",
        id: e.id,
        name: e.food_name.clone(),
        brand: e.food_brand.clone().unwrap_or_default(),
        source_label: foods::source_label(&e.source, lang),
        food_url: e.food_id.map(|id| format!("/foods/{id}")),
        unit: e.unit.clone(),
        amount: amount.to_string(),
        date: date.to_string(),
        meal: meal.to_string(),
        meals: Meal::ALL,
        per100_kcal: fmt_opt_kcal(per.energy_kcal),
        per100_protein: lang.num(fmt_opt_g(per.protein_g)),
        per100_carbs: lang.num(fmt_opt_g(per.carbs_g)),
        per100_fat: lang.num(fmt_opt_g(per.fat_g)),
        data_kcal: data_attr(per.energy_kcal),
        data_protein: data_attr(per.protein_g),
        data_carbs: data_attr(per.carbs_g),
        data_fat: data_attr(per.fat_g),
        errors,
        back_url: format!("/diary?date={}#meal-{}", e.diary_date, e.meal),
    }
}

pub async fn edit_form(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<Response> {
    let id = parse_id(&id)?;
    let e = diary::get(&state.db, id, current_lang())
        .await?
        .ok_or(AppError::NotFound)?;
    let page = edit_page(
        &e,
        &fmt_amount(e.amount),
        &e.diary_date,
        &e.meal,
        FieldErrors::default(),
        current_lang(),
    );
    Ok(render(&page)?.into_response())
}

pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Form(form): Form<EntryForm>,
) -> AppResult<Response> {
    let id = parse_id(&id)?;
    let e = diary::get(&state.db, id, current_lang())
        .await?
        .ok_or(AppError::NotFound)?;
    let lang = current_lang();
    match validate_entry(&form, lang) {
        Ok(v) => {
            diary::update(&state.db, id, v.date, v.meal, v.amount).await?;
            Ok(see_other(&diary_url(v.date, "entry_saved", v.meal)))
        }
        Err(errors) => {
            let page = edit_page(&e, &form.amount, &form.date, &form.meal, errors, lang);
            render_status(StatusCode::UNPROCESSABLE_ENTITY, &page)
        }
    }
}

pub async fn delete(State(state): State<AppState>, Path(id): Path<String>) -> AppResult<Response> {
    let id = parse_id(&id)?;
    let e = diary::get(&state.db, id, current_lang())
        .await?
        .ok_or(AppError::NotFound)?;
    diary::delete(&state.db, id).await?;
    Ok(see_other(&diary_url(
        parse_date(&e.diary_date).unwrap_or_else(|| today_in("UTC")),
        "entry_deleted",
        e.meal(),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_percentage_is_clamped() {
        assert_eq!(pct(500.0, 2000.0), 25);
        assert_eq!(pct(3000.0, 2000.0), 100);
        assert_eq!(pct(10.0, 0.0), 0);
    }
}
