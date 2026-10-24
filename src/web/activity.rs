//! Activity page: steps walked and sport sessions for a day.

use askama::Template;
use axum::Form;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use jiff::civil::Date;
use serde::Deserialize;

use super::diary::{day_title, shift};
use super::{AppState, current_lang, flash_message, parse_id, render, render_status, see_other};
use crate::activity::{
    self, Activity, NewActivity, SportKind, Weekdays, check_duration, check_steps,
};
use crate::error::{AppError, AppResult};
use crate::forms::FieldErrors;
use crate::i18n::Lang;
use crate::nutrition::fmt_kcal;
use crate::profile;
use crate::sports::{self, Intensity, Sport};
use crate::units::{
    clean_text, fmt_date_long, fmt_date_short, parse_date, today_in, weekday_long, weekday_short,
};

/// A whole number with the language's digit grouping: 12,345 / 12 345 / 12.345.
pub fn fmt_count(n: i64, lang: Lang) -> String {
    let sep = match lang {
        Lang::En => ",",
        Lang::Fr => "\u{202f}",
        Lang::Es => ".",
    };
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push_str(sep);
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}

/// How full the steps ring is, 0–100.
pub fn steps_pct(steps: i64, goal: i64) -> u32 {
    if goal <= 0 {
        return 0;
    }
    (steps as f64 / goal as f64 * 100.0)
        .clamp(0.0, 100.0)
        .round() as u32
}

/// "45 min", "1 h", "1 h 30 min".
pub fn fmt_duration(minutes: i64) -> String {
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}

/// The listed sport of a session. One recorded before the list existed is
/// matched by its name when possible.
fn matched_sport(a: &Activity) -> Option<&'static Sport> {
    a.listed_sport().or_else(|| {
        a.intensity()
            .is_none()
            .then(|| sports::find_by_name(&a.sport))
            .flatten()
    })
}

/// The value of the sport menu for a sport not in the list.
const CUSTOM: &str = "custom";

pub struct ActivityView {
    pub sport: String,
    /// Id of the sport's icon in `static/sports.svg`.
    pub icon: &'static str,
    pub duration: String,
    /// Estimated kcal, when the sport and the weight allow one.
    pub kcal: Option<String>,
    pub repeat: String,
    pub note: String,
    pub edit_url: String,
}

/// One choice of a menu.
pub struct MenuOption {
    pub value: &'static str,
    pub label: &'static str,
    /// Id of an icon in `static/sports.svg`, or empty for none.
    pub icon: &'static str,
    pub selected: bool,
}

/// One weekday toggle in the repeat picker.
pub struct DayOption {
    pub index: usize,
    pub short: &'static str,
    pub long: &'static str,
    pub checked: bool,
}

/// The activity form's values as typed, for re-display.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ActivityForm {
    /// A listed sport's key, [`CUSTOM`], or empty before a choice.
    #[serde(default)]
    pub sport_key: String,
    /// The name of a custom sport.
    #[serde(default)]
    pub sport: String,
    #[serde(default)]
    pub intensity: String,
    #[serde(default)]
    pub duration: String,
    #[serde(default)]
    pub start: String,
    #[serde(default)]
    pub repeat: String,
    #[serde(default)]
    pub end: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    day0: String,
    #[serde(default)]
    day1: String,
    #[serde(default)]
    day2: String,
    #[serde(default)]
    day3: String,
    #[serde(default)]
    day4: String,
    #[serde(default)]
    day5: String,
    #[serde(default)]
    day6: String,
}

impl ActivityForm {
    fn days(&self) -> Weekdays {
        [
            &self.day0, &self.day1, &self.day2, &self.day3, &self.day4, &self.day5, &self.day6,
        ]
        .into_iter()
        .enumerate()
        .filter(|(_, v)| !v.is_empty())
        .fold(Weekdays::NONE, |w, (i, _)| w.with(i))
    }

    fn set_days(&mut self, w: Weekdays) {
        let on = |i| {
            if w.has(i) {
                "on".to_string()
            } else {
                String::new()
            }
        };
        self.day0 = on(0);
        self.day1 = on(1);
        self.day2 = on(2);
        self.day3 = on(3);
        self.day4 = on(4);
        self.day5 = on(5);
        self.day6 = on(6);
    }

    fn repeats(&self) -> bool {
        !self.repeat.is_empty()
    }

    /// An empty form for a session on `d`; the repeat picker starts on
    /// that day's weekday.
    fn blank(d: Date) -> Self {
        let mut f = ActivityForm {
            start: d.to_string(),
            ..Default::default()
        };
        f.set_days(Weekdays::of(d));
        f
    }

    fn from_activity(a: &Activity) -> Self {
        let listed = matched_sport(a);
        let mut f = ActivityForm {
            sport_key: listed.map_or(CUSTOM, |s| s.key).into(),
            sport: if listed.is_some() {
                String::new()
            } else {
                a.sport.clone()
            },
            intensity: a.intensity.clone().unwrap_or_default(),
            duration: a.duration_min.to_string(),
            start: a.start_date.clone(),
            repeat: if a.repeats() {
                "on".into()
            } else {
                String::new()
            },
            end: a.end_date.clone().unwrap_or_default(),
            note: a.note.clone().unwrap_or_default(),
            ..Default::default()
        };
        let days = if a.repeats() {
            a.repeat()
        } else {
            parse_date(&a.start_date)
                .map(Weekdays::of)
                .unwrap_or_default()
        };
        f.set_days(days);
        f
    }

    pub fn is_custom(&self) -> bool {
        self.sport_key == CUSTOM
    }

    /// The listed sports in alphabetical order, then "Custom"; first a
    /// prompt while nothing is chosen.
    fn sport_options(&self, lang: Lang) -> Vec<MenuOption> {
        let mut v = Vec::with_capacity(sports::SPORTS.len() + 2);
        if self.sport_key.is_empty() {
            v.push(MenuOption {
                value: "",
                label: lang.t("Choose"),
                icon: "",
                selected: true,
            });
        }
        v.extend(sports::sorted(lang).into_iter().map(|s| MenuOption {
            value: s.key,
            label: s.name(lang),
            icon: s.icon(),
            selected: self.sport_key == s.key,
        }));
        v.push(MenuOption {
            value: CUSTOM,
            label: lang.t("Custom"),
            icon: sports::CUSTOM_ICON,
            selected: self.is_custom(),
        });
        v
    }

    fn intensity_options(&self, lang: Lang) -> Vec<MenuOption> {
        let mut v = Vec::with_capacity(4);
        if Intensity::parse(&self.intensity).is_none() {
            v.push(MenuOption {
                value: "",
                label: lang.t("Choose"),
                icon: "",
                selected: true,
            });
        }
        v.extend(Intensity::ALL.into_iter().map(|i| MenuOption {
            value: i.as_str(),
            label: i.label(lang),
            icon: "",
            selected: self.intensity == i.as_str(),
        }));
        v
    }

    fn day_options(&self, lang: Lang) -> Vec<DayOption> {
        let days = self.days();
        (0..7)
            .map(|i| DayOption {
                index: i,
                short: weekday_short(i, lang),
                long: weekday_long(i, lang),
                checked: days.has(i),
            })
            .collect()
    }
}

fn validate(form: &ActivityForm, lang: Lang) -> Result<NewActivity, FieldErrors> {
    let mut errors = FieldErrors::default();
    let kind = match form.sport_key.as_str() {
        "" => {
            errors.add("sport_key", lang.t("Choose a sport."));
            None
        }
        CUSTOM => {
            let name = match clean_text(&form.sport, activity::MAX_SPORT_CHARS) {
                Ok(Some(s)) => Some(s),
                Ok(None) => {
                    errors.add("sport", lang.t("Enter the name of the sport."));
                    None
                }
                Err(e) => {
                    errors.add("sport", e.for_field(lang, lang.t("Name")));
                    None
                }
            };
            let intensity = Intensity::parse(&form.intensity);
            if intensity.is_none() {
                errors.add("intensity", lang.t("Choose how intense it is."));
            }
            name.zip(intensity)
                .map(|(name, intensity)| SportKind::Custom { name, intensity })
        }
        key => match sports::find(key) {
            Some(s) => Some(SportKind::Listed(s)),
            None => {
                errors.add("sport_key", lang.t("Choose a sport."));
                None
            }
        },
    };
    let duration = check_duration(&form.duration, lang)
        .map_err(|e| errors.add("duration", e))
        .ok();
    let start = parse_date(&form.start);
    if start.is_none() {
        errors.add("start", lang.t("Choose a valid date."));
    }
    let repeat = if form.repeats() {
        let days = form.days();
        if days.is_empty() {
            errors.add("days", lang.t("Choose at least one day of the week."));
        }
        days
    } else {
        Weekdays::NONE
    };
    let end = match form.end.trim() {
        "" => None,
        _ if !form.repeats() => None,
        s => match parse_date(s) {
            Some(end) if start.is_some_and(|start| end < start) => {
                errors.add(
                    "end",
                    lang.t("The end date must be on or after the first day."),
                );
                None
            }
            Some(end) => Some(end),
            None => {
                errors.add("end", lang.t("Choose a valid date."));
                None
            }
        },
    };
    let note = clean_text(&form.note, activity::MAX_NOTE_CHARS).unwrap_or_else(|e| {
        errors.add("note", e.for_field(lang, lang.t("Note")));
        None
    });
    match (kind, duration, start) {
        (Some(kind), Some(duration_min), Some(start)) if errors.is_empty() => Ok(NewActivity {
            kind,
            duration_min,
            note,
            start,
            repeat,
            end,
        }),
        _ => Err(errors),
    }
}

#[derive(Template)]
#[template(path = "activity.html")]
pub struct ActivityPage {
    pub lang: Lang,
    pub title: String,
    pub nav: &'static str,
    pub day_path: &'static str,
    pub date_field_label: &'static str,
    pub date_iso: String,
    pub date_label: String,
    pub prev_iso: String,
    pub next_iso: String,
    pub today_iso: String,
    pub is_today: bool,
    /// Steps entered for the day, formatted; `None` when none were.
    pub steps: Option<String>,
    pub step_goal: String,
    /// Share of the step goal reached, 0–100, for the ring.
    pub steps_pct: u32,
    /// Estimated kcal of the day's steps, when height and weight are known.
    pub steps_kcal: Option<String>,
    pub steps_input: String,
    pub activities: Vec<ActivityView>,
    pub total_duration: String,
    /// Estimated kcal of the day's sport, when any session has an estimate.
    pub sport_kcal: Option<String>,
    /// Sessions exist but no weight is known to estimate their energy.
    pub sport_needs_weight: bool,
    pub form: ActivityForm,
    pub sports: Vec<MenuOption>,
    pub intensities: Vec<MenuOption>,
    pub days: Vec<DayOption>,
    pub errors: FieldErrors,
    pub message: Option<&'static str>,
}

/// Values typed into the page's forms, re-displayed after an error.
#[derive(Default)]
struct Typed {
    steps: Option<String>,
    activity: Option<ActivityForm>,
}

async fn activity_page(
    state: &AppState,
    date: Date,
    typed: Typed,
    errors: FieldErrors,
    message: Option<&'static str>,
) -> AppResult<ActivityPage> {
    let lang = current_lang();
    let p = profile::load(&state.db).await?;
    let today = today_in(&p.timezone);
    let entered = activity::steps_on(&state.db, date).await?;
    let energy = activity::step_energy_on(&state.db, date).await?;
    let list = activity::for_date(&state.db, date).await?;
    let total: i64 = list.iter().map(|a| a.duration_min).sum();
    let weight = if list.is_empty() {
        None
    } else {
        activity::weight_on(&state.db, date).await?
    };
    let sport_energy = weight
        .map(|w| activity::sport_energy(&list, w))
        .filter(|e| e.minutes > 0);
    let activities = list
        .iter()
        .map(|a| ActivityView {
            sport: a.name(lang).to_string(),
            icon: matched_sport(a).map_or(sports::CUSTOM_ICON, Sport::icon),
            duration: fmt_duration(a.duration_min),
            kcal: weight.and_then(|w| a.kcal(w)).map(fmt_kcal),
            repeat: if a.repeats() {
                a.repeat().label(lang)
            } else {
                String::new()
            },
            note: a.note.clone().unwrap_or_default(),
            edit_url: format!("/activity/{}/edit?date={date}", a.id),
        })
        .collect();
    let form = typed.activity.unwrap_or_else(|| ActivityForm::blank(date));
    Ok(ActivityPage {
        lang,
        title: day_title(date, today, lang),
        nav: "activity",
        day_path: "/activity",
        date_field_label: lang.t("Activity date"),
        date_iso: date.to_string(),
        date_label: fmt_date_long(date, lang),
        prev_iso: shift(date, -1).to_string(),
        next_iso: shift(date, 1).to_string(),
        today_iso: today.to_string(),
        is_today: date == today,
        steps: entered.map(|n| fmt_count(n, lang)),
        step_goal: fmt_count(p.step_goal, lang),
        steps_pct: steps_pct(entered.unwrap_or(0), p.step_goal),
        steps_kcal: energy.map(|e| fmt_kcal(e.kcal)),
        steps_input: typed
            .steps
            .unwrap_or_else(|| entered.map(|n| n.to_string()).unwrap_or_default()),
        activities,
        total_duration: if total > 0 {
            fmt_duration(total)
        } else {
            String::new()
        },
        sport_kcal: sport_energy.map(|e| fmt_kcal(e.kcal)),
        sport_needs_weight: !list.is_empty() && weight.is_none(),
        sports: form.sport_options(lang),
        intensities: form.intensity_options(lang),
        days: form.day_options(lang),
        form,
        errors,
        message,
    })
}

/// The page's date: the given one when valid, otherwise today.
async fn day_or_today(state: &AppState, s: Option<&str>) -> AppResult<Date> {
    match s.and_then(parse_date) {
        Some(d) => Ok(d),
        None => Ok(today_in(&profile::load(&state.db).await?.timezone)),
    }
}

fn day_url(date: Date, msg: &str) -> String {
    format!("/activity?date={date}&msg={msg}")
}

#[derive(Deserialize)]
pub struct DayQuery {
    date: Option<String>,
    msg: Option<String>,
}

pub async fn page(State(state): State<AppState>, Query(q): Query<DayQuery>) -> AppResult<Response> {
    let lang = current_lang();
    let date = match q.date.as_deref() {
        None | Some("") => day_or_today(&state, None).await?,
        Some(s) => parse_date(s).ok_or_else(|| {
            AppError::BadRequest(
                lang.t("The date must be a valid calendar date (YYYY-MM-DD).")
                    .into(),
            )
        })?,
    };
    let page = activity_page(
        &state,
        date,
        Typed::default(),
        FieldErrors::default(),
        flash_message(q.msg.as_deref(), lang),
    )
    .await?;
    Ok(render(&page)?.into_response())
}

#[derive(Deserialize)]
pub struct StepsForm {
    #[serde(default)]
    date: String,
    #[serde(default)]
    steps: String,
}

pub async fn save_steps(
    State(state): State<AppState>,
    Form(form): Form<StepsForm>,
) -> AppResult<Response> {
    let lang = current_lang();
    let date = parse_date(&form.date).ok_or_else(|| {
        AppError::BadRequest(
            lang.t("The date must be a valid calendar date (YYYY-MM-DD).")
                .into(),
        )
    })?;
    match check_steps(&form.steps, lang) {
        Ok(n) => {
            activity::set_steps(&state.db, date, n).await?;
            Ok(see_other(&day_url(date, "steps_saved")))
        }
        Err(e) => {
            let mut errors = FieldErrors::default();
            errors.add("steps", e);
            let typed = Typed {
                steps: Some(form.steps),
                ..Default::default()
            };
            let page = activity_page(&state, date, typed, errors, None).await?;
            render_status(StatusCode::UNPROCESSABLE_ENTITY, &page)
        }
    }
}

#[derive(Deserialize)]
pub struct DateForm {
    #[serde(default)]
    date: String,
}

pub async fn clear_steps(
    State(state): State<AppState>,
    Form(form): Form<DateForm>,
) -> AppResult<Response> {
    let lang = current_lang();
    let date = parse_date(&form.date).ok_or_else(|| {
        AppError::BadRequest(
            lang.t("The date must be a valid calendar date (YYYY-MM-DD).")
                .into(),
        )
    })?;
    activity::clear_steps(&state.db, date).await?;
    Ok(see_other(&day_url(date, "steps_deleted")))
}

pub async fn create(
    State(state): State<AppState>,
    Form(form): Form<ActivityForm>,
) -> AppResult<Response> {
    let lang = current_lang();
    match validate(&form, lang) {
        Ok(a) => {
            activity::create(&state.db, &a).await?;
            Ok(see_other(&day_url(a.start, "activity_added")))
        }
        Err(errors) => {
            let date = day_or_today(&state, Some(&form.start)).await?;
            let typed = Typed {
                activity: Some(form),
                ..Default::default()
            };
            let page = activity_page(&state, date, typed, errors, None).await?;
            render_status(StatusCode::UNPROCESSABLE_ENTITY, &page)
        }
    }
}

#[derive(Template)]
#[template(path = "activity_edit.html")]
pub struct ActivityEditPage {
    pub lang: Lang,
    pub title: String,
    pub nav: &'static str,
    pub id: i64,
    pub back_url: String,
    /// The day the activity was opened from.
    pub date_iso: String,
    /// A repeating activity can be skipped on the day it was opened from.
    pub skip_label: Option<String>,
    pub repeats: bool,
    pub form: ActivityForm,
    pub sports: Vec<MenuOption>,
    pub intensities: Vec<MenuOption>,
    pub days: Vec<DayOption>,
    pub errors: FieldErrors,
}

fn edit_page(
    a: &Activity,
    date: Option<Date>,
    form: ActivityForm,
    errors: FieldErrors,
    lang: Lang,
) -> ActivityEditPage {
    let back_date = date.or_else(|| parse_date(&a.start_date));
    ActivityEditPage {
        lang,
        title: lang.f1("Edit {0}", a.name(lang)),
        nav: "activity",
        id: a.id,
        back_url: match back_date {
            Some(d) => format!("/activity?date={d}"),
            None => "/activity".into(),
        },
        date_iso: back_date.map(|d| d.to_string()).unwrap_or_default(),
        skip_label: date
            .filter(|d| a.repeats() && a.occurs_on(*d))
            .map(|d| lang.f1("Skip on {0} only", fmt_date_short(d, lang))),
        repeats: a.repeats(),
        sports: form.sport_options(lang),
        intensities: form.intensity_options(lang),
        days: form.day_options(lang),
        form,
        errors,
    }
}

#[derive(Deserialize)]
pub struct EditQuery {
    date: Option<String>,
}

async fn load(state: &AppState, id: &str) -> AppResult<Activity> {
    let id = parse_id(id)?;
    activity::get(&state.db, id)
        .await?
        .ok_or(AppError::NotFound)
}

pub async fn edit_form(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<EditQuery>,
) -> AppResult<Response> {
    let a = load(&state, &id).await?;
    let date = q.date.as_deref().and_then(parse_date);
    let page = edit_page(
        &a,
        date,
        ActivityForm::from_activity(&a),
        FieldErrors::default(),
        current_lang(),
    );
    Ok(render(&page)?.into_response())
}

#[derive(Deserialize)]
pub struct EditForm {
    #[serde(flatten)]
    form: ActivityForm,
    /// The day the edit page was opened from.
    #[serde(default)]
    from: String,
}

pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Form(edit): Form<EditForm>,
) -> AppResult<Response> {
    let a = load(&state, &id).await?;
    let lang = current_lang();
    let from = parse_date(&edit.from);
    match validate(&edit.form, lang) {
        Ok(v) => {
            if !activity::update(&state.db, a.id, &v).await? {
                return Err(AppError::NotFound);
            }
            // Back to the day it was opened from if it still takes place
            // there, otherwise to its first day.
            let changed = Activity {
                start_date: v.start.to_string(),
                weekdays: v.repeat.bits(),
                end_date: v.end.map(|d| d.to_string()),
                ..a
            };
            let date = from.filter(|d| changed.occurs_on(*d)).unwrap_or(v.start);
            Ok(see_other(&day_url(date, "activity_saved")))
        }
        Err(errors) => {
            let page = edit_page(&a, from, edit.form, errors, lang);
            render_status(StatusCode::UNPROCESSABLE_ENTITY, &page)
        }
    }
}

pub async fn skip(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Form(form): Form<DateForm>,
) -> AppResult<Response> {
    let a = load(&state, &id).await?;
    let lang = current_lang();
    let date = parse_date(&form.date)
        .filter(|d| a.repeats() && a.occurs_on(*d))
        .ok_or_else(|| {
            AppError::BadRequest(lang.t("This activity does not repeat on that day.").into())
        })?;
    activity::skip(&state.db, a.id, date).await?;
    Ok(see_other(&day_url(date, "activity_skipped")))
}

pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Form(form): Form<DateForm>,
) -> AppResult<Response> {
    let a = load(&state, &id).await?;
    activity::delete(&state.db, a.id).await?;
    let date = match parse_date(&form.date).or_else(|| parse_date(&a.start_date)) {
        Some(d) => d,
        None => day_or_today(&state, None).await?,
    };
    Ok(see_other(&day_url(date, "activity_deleted")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    #[test]
    fn counts_are_grouped_per_language() {
        assert_eq!(fmt_count(5000, Lang::En), "5,000");
        assert_eq!(fmt_count(12345, Lang::Fr), "12\u{202f}345");
        assert_eq!(fmt_count(1_234_567, Lang::Es), "1.234.567");
        assert_eq!(fmt_count(999, Lang::En), "999");
        assert_eq!(fmt_count(0, Lang::En), "0");
    }

    #[test]
    fn steps_ring_is_clamped() {
        assert_eq!(steps_pct(5_000, 10_000), 50);
        assert_eq!(steps_pct(14_000, 10_000), 100);
        assert_eq!(steps_pct(0, 10_000), 0);
        assert_eq!(steps_pct(10, 0), 0);
    }

    #[test]
    fn durations_read_as_hours_and_minutes() {
        assert_eq!(fmt_duration(45), "45 min");
        assert_eq!(fmt_duration(60), "1 h");
        assert_eq!(fmt_duration(95), "1 h 35 min");
    }

    /// Decodes form fields the way the `Form` extractor does.
    fn form(pairs: &str) -> ActivityForm {
        let uri: axum::http::Uri = format!("/?{pairs}").parse().unwrap();
        Query::<ActivityForm>::try_from_uri(&uri).unwrap().0
    }

    #[test]
    fn once_ignores_weekdays_and_end() {
        let v = validate(
            &form("sport_key=yoga&duration=60&start=2026-10-24&day2=on&end=2026-09-01"),
            Lang::En,
        )
        .unwrap();
        assert_eq!(v.repeat, Weekdays::NONE);
        assert_eq!(v.end, None);
    }

    #[test]
    fn repeating_needs_days_and_a_later_end() {
        let v = validate(
            &form("sport_key=running&duration=1%2C000&start=2026-10-24&repeat=on&day0=on&day3=on"),
            Lang::En,
        );
        assert!(v.is_ok(), "1,000 minutes is within a day");
        let v = validate(
            &form(
                "sport_key=running&duration=30&start=2026-10-24&repeat=on&day0=on&end=2026-12-31",
            ),
            Lang::En,
        )
        .unwrap();
        assert_eq!(v.repeat, Weekdays::NONE.with(0));
        assert_eq!(v.end, Some(date(2026, 12, 31)));

        let e = validate(
            &form("sport_key=running&duration=30&start=2026-10-24&repeat=on"),
            Lang::En,
        )
        .unwrap_err();
        assert!(e.has("days"));
        let e = validate(
            &form(
                "sport_key=running&duration=30&start=2026-10-24&repeat=on&day1=on&end=2026-10-01",
            ),
            Lang::En,
        )
        .unwrap_err();
        assert!(e.has("end"));
    }

    #[test]
    fn required_fields_are_reported_together() {
        let e = validate(&form("sport_key=&duration=0&start=nope"), Lang::En).unwrap_err();
        assert!(e.has("sport_key") && e.has("duration") && e.has("start"));
    }

    #[test]
    fn custom_sport_needs_a_name_and_an_intensity() {
        let e = validate(
            &form("sport_key=custom&sport=+&duration=30&start=2026-10-24"),
            Lang::En,
        )
        .unwrap_err();
        assert!(e.has("sport") && e.has("intensity"));
        assert!(
            validate(
                &form("sport_key=quidditch&duration=30&start=2026-10-24"),
                Lang::En
            )
            .unwrap_err()
            .has("sport_key")
        );
        let v = validate(
            &form("sport_key=custom&sport=Parkour&intensity=vigorous&duration=30&start=2026-10-24"),
            Lang::En,
        )
        .unwrap();
        assert_eq!(
            v.kind,
            SportKind::Custom {
                name: "Parkour".into(),
                intensity: Intensity::Vigorous
            }
        );
        // A listed sport ignores a leftover custom name and intensity.
        let v = validate(
            &form("sport_key=yoga&sport=X&intensity=light&duration=30&start=2026-10-24"),
            Lang::En,
        )
        .unwrap();
        assert_eq!(v.kind, SportKind::Listed(sports::find("yoga").unwrap()));
    }

    #[test]
    fn older_sessions_are_matched_to_a_listed_sport_by_name() {
        let mut a = Activity {
            id: 1,
            sport: "Natation".into(),
            sport_key: None,
            intensity: None,
            duration_min: 45,
            note: None,
            start_date: "2026-10-24".into(),
            weekdays: 0,
            end_date: None,
        };
        let f = ActivityForm::from_activity(&a);
        assert_eq!((f.sport_key.as_str(), f.sport.as_str()), ("swimming", ""));
        a.sport = "Parkour".into();
        let f = ActivityForm::from_activity(&a);
        assert_eq!(
            (f.sport_key.as_str(), f.sport.as_str()),
            (CUSTOM, "Parkour")
        );
        assert_eq!(
            f.intensity_options(Lang::En)[0].value,
            "",
            "still to choose"
        );
    }

    #[test]
    fn a_blank_form_suggests_the_days_weekday() {
        // 24 October 2026 is a Saturday.
        let f = ActivityForm::blank(date(2026, 10, 24));
        assert_eq!(f.days(), Weekdays::NONE.with(5));
        assert!(!f.repeats());
    }
}
