//! Weight log, history and trend chart.

use askama::Template;
use axum::Form;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use jiff::civil::Date;
use serde::Deserialize;

use super::{AppState, current_lang, flash_message, parse_id, render, render_status, see_other};
use crate::error::{AppError, AppResult};
use crate::forms::FieldErrors;
use crate::i18n::Lang;
use crate::profile;
use crate::units::{UnitSystem, clean_text, fmt_date_short, parse_date, parse_decimal, today_in};
use crate::weight::{self, Saved, UpdateResult, WeightEntry, check_weight_kg};

/// Number of most recent weigh-ins drawn in the chart.
const CHART_POINTS: usize = 90;

pub struct WeightRow {
    pub id: i64,
    pub date: String,
    pub date_label: String,
    pub weight: String,
    pub change: String,
    pub note: String,
}

pub struct ChartPoint {
    pub x: String,
    pub y: String,
    pub label: String,
}

pub struct ChartTick {
    pub pos: String,
    pub label: String,
}

pub struct Chart {
    pub width: u32,
    pub height: u32,
    pub points: String,
    pub dots: Vec<ChartPoint>,
    pub y_ticks: Vec<ChartTick>,
    pub x_ticks: Vec<ChartTick>,
    pub summary: String,
    pub plot_left: u32,
    pub plot_right: u32,
    pub plot_bottom: u32,
}

#[derive(Template)]
#[template(path = "weight.html")]
pub struct WeightPage {
    pub lang: Lang,
    pub title: String,
    pub nav: &'static str,
    pub unit: &'static str,
    pub rows: Vec<WeightRow>,
    pub chart: Option<Chart>,
    pub date: String,
    pub weight: String,
    pub note: String,
    pub errors: FieldErrors,
    pub message: Option<&'static str>,
}

fn fmt_weight(units: UnitSystem, kg: f64) -> String {
    format!("{:.1}", units.weight_from_kg(kg))
}

/// One decimal place with the language's decimal separator.
fn fmt_1(v: f64, lang: Lang) -> String {
    lang.num(format!("{v:.1}"))
}

/// Builds an SVG line chart from entries in ascending date order.
pub fn build_chart(entries: &[WeightEntry], units: UnitSystem, lang: Lang) -> Option<Chart> {
    if entries.len() < 2 {
        return None;
    }
    let (width, height) = (640u32, 260u32);
    let (left, right, top, bottom) = (48.0, 624.0, 16.0, 220.0);

    let dates: Vec<Date> = entries
        .iter()
        .filter_map(|e| parse_date(&e.entry_date))
        .collect();
    if dates.len() != entries.len() {
        return None;
    }
    let values: Vec<f64> = entries
        .iter()
        .map(|e| units.weight_from_kg(e.weight_kg))
        .collect();
    let (mut lo, mut hi) = values
        .iter()
        .fold((f64::MAX, f64::MIN), |(l, h), v| (l.min(*v), h.max(*v)));
    // Pad the range so a flat line is not drawn on the edge.
    let pad = ((hi - lo) * 0.1).max(1.0);
    lo -= pad;
    hi += pad;

    let first = dates[0];
    let span_days = (dates[dates.len() - 1] - first).get_days().max(1) as f64;
    let x_of = |d: Date| left + ((d - first).get_days() as f64 / span_days) * (right - left);
    let y_of = |v: f64| bottom - (v - lo) / (hi - lo) * (bottom - top);

    let mut points = String::new();
    let mut dots = Vec::with_capacity(entries.len());
    for (d, v) in dates.iter().zip(&values) {
        let (x, y) = (x_of(*d), y_of(*v));
        points.push_str(&format!("{x:.1},{y:.1} "));
        dots.push(ChartPoint {
            x: format!("{x:.1}"),
            y: format!("{y:.1}"),
            label: format!(
                "{}: {} {}",
                fmt_date_short(*d, lang),
                fmt_1(*v, lang),
                units.weight_unit()
            ),
        });
    }

    let y_ticks = (0..=4)
        .map(|i| {
            let v = lo + (hi - lo) * f64::from(i) / 4.0;
            ChartTick {
                pos: format!("{:.1}", y_of(v)),
                label: fmt_1(v, lang),
            }
        })
        .collect();
    let last = dates[dates.len() - 1];
    let x_ticks = vec![
        ChartTick {
            pos: format!("{left:.1}"),
            label: fmt_date_short(first, lang),
        },
        ChartTick {
            pos: format!("{right:.1}"),
            label: fmt_date_short(last, lang),
        },
    ];
    let change = values[values.len() - 1] - values[0];
    let unit = units.weight_unit();
    let summary = lang.f(
        "Weight from {0} to {1}: {2} entries, from {3} to {4} ({5}).",
        &[
            &fmt_date_short(first, lang),
            &fmt_date_short(last, lang),
            &values.len(),
            &format!("{} {unit}", fmt_1(values[0], lang)),
            &format!("{} {unit}", fmt_1(values[values.len() - 1], lang)),
            &format!(
                "{}{} {unit}",
                if change >= 0.0 { "+" } else { "" },
                fmt_1(change, lang)
            ),
        ],
    );
    Some(Chart {
        width,
        height,
        points: points.trim_end().to_string(),
        dots,
        y_ticks,
        x_ticks,
        summary,
        plot_left: left as u32,
        plot_right: right as u32,
        plot_bottom: bottom as u32,
    })
}

async fn weight_page(
    state: &AppState,
    date: String,
    weight_s: String,
    note: String,
    errors: FieldErrors,
    message: Option<&'static str>,
) -> AppResult<WeightPage> {
    let lang = current_lang();
    let p = profile::load(&state.db).await?;
    let units = p.units();
    let history = weight::history(&state.db).await?;
    let rows = history
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let change = history
                .get(i + 1)
                .map(|prev| {
                    let d =
                        units.weight_from_kg(e.weight_kg) - units.weight_from_kg(prev.weight_kg);
                    format!("{}{}", if d > 0.0 { "+" } else { "" }, fmt_1(d, lang))
                })
                .unwrap_or_default();
            WeightRow {
                id: e.id,
                date: e.entry_date.clone(),
                date_label: parse_date(&e.entry_date)
                    .map(|d| fmt_date_short(d, lang))
                    .unwrap_or_else(|| e.entry_date.clone()),
                weight: lang.num(fmt_weight(units, e.weight_kg)),
                change,
                note: e.note.clone().unwrap_or_default(),
            }
        })
        .collect();
    let mut recent: Vec<WeightEntry> = history.iter().take(CHART_POINTS).cloned().collect();
    recent.reverse();
    Ok(WeightPage {
        lang,
        title: lang.t("Weight").into(),
        nav: "weight",
        unit: units.weight_unit(),
        rows,
        chart: build_chart(&recent, units, lang),
        date: if date.is_empty() {
            today_in(&p.timezone).to_string()
        } else {
            date
        },
        weight: weight_s,
        note,
        errors,
        message,
    })
}

#[derive(Deserialize)]
pub struct MsgQuery {
    msg: Option<String>,
}

pub async fn page(State(state): State<AppState>, Query(q): Query<MsgQuery>) -> AppResult<Response> {
    let page = weight_page(
        &state,
        String::new(),
        String::new(),
        String::new(),
        FieldErrors::default(),
        flash_message(q.msg.as_deref(), current_lang()),
    )
    .await?;
    Ok(render(&page)?.into_response())
}

#[derive(Deserialize)]
pub struct WeightForm {
    #[serde(default)]
    date: String,
    #[serde(default)]
    weight: String,
    #[serde(default)]
    note: String,
}

struct ValidWeight {
    date: Date,
    kg: f64,
    note: Option<String>,
}

fn validate(form: &WeightForm, units: UnitSystem, lang: Lang) -> Result<ValidWeight, FieldErrors> {
    let mut errors = FieldErrors::default();
    let date = parse_date(&form.date);
    if date.is_none() {
        errors.add("date", lang.t("Choose a valid date."));
    }
    let kg = match parse_decimal(&form.weight) {
        Ok(Some(v)) => match check_weight_kg(units.weight_to_kg(v), lang) {
            Ok(kg) => Some(kg),
            Err(e) => {
                errors.add("weight", e);
                None
            }
        },
        Ok(None) => {
            errors.add("weight", lang.t("Enter your weight."));
            None
        }
        Err(e) => {
            errors.add("weight", e.for_field(lang, lang.t("Weight")));
            None
        }
    };
    let note = clean_text(&form.note, 500).unwrap_or_else(|e| {
        errors.add("note", e.for_field(lang, lang.t("Note")));
        None
    });
    match (date, kg) {
        (Some(date), Some(kg)) if errors.is_empty() => Ok(ValidWeight { date, kg, note }),
        _ => Err(errors),
    }
}

pub async fn create(
    State(state): State<AppState>,
    Form(form): Form<WeightForm>,
) -> AppResult<Response> {
    let units = profile::load(&state.db).await?.units();
    match validate(&form, units, current_lang()) {
        Ok(v) => {
            let saved = weight::record(&state.db, v.date, v.kg, v.note.as_deref()).await?;
            Ok(see_other(match saved {
                Saved::Created => "/weight?msg=weight_saved",
                Saved::Replaced => "/weight?msg=weight_replaced",
            }))
        }
        Err(errors) => {
            let page = weight_page(&state, form.date, form.weight, form.note, errors, None).await?;
            render_status(StatusCode::UNPROCESSABLE_ENTITY, &page)
        }
    }
}

#[derive(Template)]
#[template(path = "weight_edit.html")]
pub struct WeightEditPage {
    pub lang: Lang,
    pub title: String,
    pub nav: &'static str,
    pub id: i64,
    pub unit: &'static str,
    pub date: String,
    pub weight: String,
    pub note: String,
    pub errors: FieldErrors,
}

pub async fn edit_form(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<Response> {
    let id = parse_id(&id)?;
    let e = weight::get(&state.db, id)
        .await?
        .ok_or(AppError::NotFound)?;
    let units = profile::load(&state.db).await?.units();
    let lang = current_lang();
    let page = WeightEditPage {
        lang,
        title: lang.t("Edit weight").into(),
        nav: "weight",
        id,
        unit: units.weight_unit(),
        date: e.entry_date,
        weight: fmt_weight(units, e.weight_kg),
        note: e.note.unwrap_or_default(),
        errors: FieldErrors::default(),
    };
    Ok(render(&page)?.into_response())
}

pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Form(form): Form<WeightForm>,
) -> AppResult<Response> {
    let id = parse_id(&id)?;
    let units = profile::load(&state.db).await?.units();
    let lang = current_lang();
    let mut errors = match validate(&form, units, lang) {
        Ok(v) => match weight::update(&state.db, id, v.date, v.kg, v.note.as_deref()).await? {
            UpdateResult::Updated => return Ok(see_other("/weight?msg=weight_saved")),
            UpdateResult::NotFound => return Err(AppError::NotFound),
            UpdateResult::DateTaken => FieldErrors::default(),
        },
        Err(errors) => errors,
    };
    if errors.is_empty() {
        errors.add(
            "date",
            lang.t("There is already a weight for this date. Edit that entry instead."),
        );
    }
    let page = WeightEditPage {
        lang,
        title: lang.t("Edit weight").into(),
        nav: "weight",
        id,
        unit: units.weight_unit(),
        date: form.date,
        weight: form.weight,
        note: form.note,
        errors,
    };
    render_status(StatusCode::UNPROCESSABLE_ENTITY, &page)
}

pub async fn delete(State(state): State<AppState>, Path(id): Path<String>) -> AppResult<Response> {
    let id = parse_id(&id)?;
    if !weight::delete(&state.db, id).await? {
        return Err(AppError::NotFound);
    }
    Ok(see_other("/weight?msg=weight_deleted"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(date: &str, kg: f64) -> WeightEntry {
        WeightEntry {
            id: 1,
            entry_date: date.into(),
            weight_kg: kg,
            note: None,
        }
    }

    #[test]
    fn chart_needs_two_points() {
        assert!(build_chart(&[entry("2026-01-01", 80.0)], UnitSystem::Metric, Lang::En).is_none());
    }

    #[test]
    fn chart_spans_plot_area_and_describes_change() {
        let c = build_chart(
            &[
                entry("2026-01-01", 80.0),
                entry("2026-01-11", 79.0),
                entry("2026-01-21", 78.5),
            ],
            UnitSystem::Metric,
            Lang::En,
        )
        .unwrap();
        assert_eq!(c.dots.len(), 3);
        assert_eq!(c.dots[0].x, "48.0");
        assert_eq!(c.dots[2].x, "624.0");
        assert!(c.summary.contains("-1.5 kg"), "{}", c.summary);
    }

    #[test]
    fn chart_text_follows_language() {
        let c = build_chart(
            &[entry("2026-01-01", 80.0), entry("2026-02-01", 78.5)],
            UnitSystem::Metric,
            Lang::Fr,
        )
        .unwrap();
        assert!(c.summary.contains("1 janv. 2026"), "{}", c.summary);
        assert!(c.summary.contains("-1,5 kg"), "{}", c.summary);
    }

    #[test]
    fn chart_uses_display_units() {
        let c = build_chart(
            &[entry("2026-01-01", 100.0), entry("2026-01-02", 100.0)],
            UnitSystem::Imperial,
            Lang::En,
        )
        .unwrap();
        assert!(c.summary.contains("220.5 lb"), "{}", c.summary);
    }
}
