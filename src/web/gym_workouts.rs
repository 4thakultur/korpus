//! Workouts: starting one from a routine or empty, logging its sets,
//! finishing it, and the history of finished workouts.

use std::collections::HashMap;

use askama::Template;
use axum::Form;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use super::activity::fmt_count;
use super::gym::{NAV, PickFor, PickQuery, exercises_url, fmt_load_in, load_routine, picked};
use super::{AppState, current_lang, flash_message, parse_id, render, render_status, see_other};
use crate::error::{AppError, AppResult};
use crate::forms::FieldErrors;
use crate::gym::exercises::{self, Equipment};
use crate::gym::workouts::{
    self, Details, Done, ExerciseKey, Finished, Plan, SetValues, StartExercise, Started, Workout,
    WorkoutExercise, WorkoutSet,
};
use crate::gym::{self, ParseError, Target};
use crate::i18n::Lang;
use crate::profile;
use crate::units::{
    UnitSystem, clean_text, fmt_date_long, fmt_date_short, parse_date, parse_decimal, today_in,
};

/// Finished workouts per history page.
const PER_PAGE: i64 = 20;
/// Sent by the page script when it saves while the user types; the answer
/// is then a status code instead of a page.
const AUTOSAVE_HEADER: &str = "x-korpus-autosave";

#[derive(Deserialize)]
pub struct MsgQuery {
    msg: Option<String>,
}

async fn load(state: &AppState, id: &str) -> AppResult<Workout> {
    workouts::get(&state.db, parse_id(id)?)
        .await?
        .ok_or(AppError::NotFound)
}

fn workout_url(w: &Workout) -> String {
    if w.is_open() {
        format!("/gym/workouts/{}", w.id)
    } else {
        format!("/gym/workouts/{}/edit", w.id)
    }
}

// ---------- Starting ----------

#[derive(Default, Deserialize)]
pub struct StartForm {
    /// The routine to follow; empty for a workout without one.
    #[serde(default)]
    routine: String,
}

pub async fn start(
    State(state): State<AppState>,
    Form(form): Form<StartForm>,
) -> AppResult<Response> {
    let lang = current_lang();
    let today = today_in(&profile::load(&state.db).await?.timezone).to_string();
    let (name, routine, list) = if form.routine.is_empty() {
        (lang.t("Workout").to_string(), None, Vec::new())
    } else {
        let (r, _) = load_routine(&state, &form.routine).await?;
        let mut list = Vec::new();
        for item in gym::items(&state.db, r.id).await? {
            if let Some(exercise) = gym::exercise_of(&state.db, &item).await? {
                list.push(StartExercise {
                    exercise,
                    plan: Some(Plan::of(&item)),
                    rest_s: item.rest_s,
                });
            }
        }
        (r.name.clone(), Some(r), list)
    };
    Ok(see_other(
        &match workouts::start(&state.db, &today, &name, routine.as_ref(), &list, lang).await? {
            Started::Created(id) => format!("/gym/workouts/{id}"),
            Started::AlreadyOpen(id) => format!("/gym/workouts/{id}?msg=workout_open"),
        },
    ))
}

/// Adds the picked exercise to a workout in progress and returns where to
/// go next.
pub(super) async fn add_picked(
    state: &AppState,
    workout_id: i64,
    q: PickQuery,
    lang: Lang,
) -> AppResult<String> {
    let w = workouts::get(&state.db, workout_id)
        .await?
        .ok_or(AppError::NotFound)?;
    if !w.is_open() {
        return Ok(format!("/gym/workouts/{}", w.id));
    }
    if workouts::exercise_count(&state.db, w.id).await? >= workouts::MAX_WORKOUT_EXERCISES {
        return Err(AppError::BadRequest(lang.f1(
            "A workout can hold up to {0} exercises.",
            workouts::MAX_WORKOUT_EXERCISES,
        )));
    }
    let ex = picked(state, &q).await?;
    let id = workouts::add_exercise(&state.db, &w, ex, lang).await?;
    Ok(format!("/gym/workouts/{}#exercise-{id}", w.id))
}

pub async fn add_exercise(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<PickQuery>,
) -> AppResult<Response> {
    let to = add_picked(&state, parse_id(&id)?, q, current_lang()).await?;
    Ok(see_other(&to))
}

// ---------- Logging ----------

pub struct SetRow {
    pub id: i64,
    pub number: usize,
    /// Last time's numbers for this set, e.g. "60 × 10".
    pub previous: String,
    pub weight: String,
    pub reps: String,
    pub duration: String,
    pub done: bool,
    pub weight_error: Option<String>,
    pub reps_error: Option<String>,
    pub duration_error: Option<String>,
}

pub struct ExerciseBlock {
    pub id: i64,
    pub name: String,
    pub timed: bool,
    pub bodyweight: bool,
    /// "3 × 8–12 reps · 60 kg · Rest 2 min", or empty without a plan.
    pub plan: String,
    /// Last time every planned set reached the top of the rep range.
    pub progress: bool,
    /// Seconds of rest after a set, for the rest timer; 0 for none.
    pub rest_s: i64,
    pub sets: Vec<SetRow>,
    pub can_add_set: bool,
}

#[derive(Template)]
#[template(path = "gym_workout.html")]
pub struct WorkoutPage {
    pub lang: Lang,
    pub title: String,
    pub nav: &'static str,
    pub message: Option<&'static str>,
    pub id: i64,
    pub open: bool,
    pub action: String,
    pub cancel_url: String,
    pub unit: &'static str,
    pub date: String,
    pub name: String,
    pub note: String,
    pub errors: FieldErrors,
    /// Every error, linked to its field, for the summary at the top.
    pub summary: Vec<(String, String)>,
    pub exercises: Vec<ExerciseBlock>,
    pub can_add_exercise: bool,
}

/// What the user typed for a workout, as the form posts it.
#[derive(Default)]
pub struct Typed {
    pub date: String,
    pub name: String,
    pub note: String,
    /// Field name (`w12`, `r12`, `d12`, `done12`) to value.
    pub fields: HashMap<String, String>,
}

impl Typed {
    fn from_form(pairs: Vec<(String, String)>) -> (Self, String) {
        let mut t = Typed::default();
        let mut action = String::new();
        for (k, v) in pairs {
            match k.as_str() {
                "date" => t.date = v,
                "name" => t.name = v,
                "note" => t.note = v,
                "action" => action = v,
                _ => {
                    t.fields.insert(k, v);
                }
            }
        }
        (t, action)
    }

    fn of(w: &Workout) -> Self {
        Typed {
            date: w.workout_date.clone(),
            name: w.name.clone(),
            note: w.note.clone().unwrap_or_default(),
            fields: HashMap::new(),
        }
    }
}

/// A set field the user typed that could not be read.
#[derive(Debug, PartialEq)]
pub struct SetError {
    pub set_id: i64,
    /// 'w' weight, 'r' reps or 'd' duration.
    pub field: char,
    pub message: String,
}

/// Reads the typed values of the sets present in the form. In a finished
/// workout every set counts as done.
pub fn read_sets(
    fields: &HashMap<String, String>,
    sets: &[WorkoutSet],
    timed: &HashMap<i64, bool>,
    finished: bool,
    units: UnitSystem,
    lang: Lang,
) -> (Vec<SetValues>, Vec<SetError>) {
    let mut values = Vec::new();
    let mut errors = Vec::new();
    for s in sets {
        let Some(weight) = fields.get(&format!("w{}", s.id)) else {
            continue;
        };
        let is_timed = timed.get(&s.workout_exercise_id).copied().unwrap_or(false);
        let done = finished || fields.contains_key(&format!("done{}", s.id));
        let mut err = |field: char, message: String| {
            errors.push(SetError {
                set_id: s.id,
                field,
                message,
            })
        };
        let weight_kg = match parse_decimal(weight) {
            Ok(None) => None,
            Ok(Some(v)) if (0.0..=gym::MAX_LOAD_KG).contains(&units.weight_to_kg(v)) => {
                Some(units.weight_to_kg(v))
            }
            Ok(Some(_)) => {
                err(
                    'w',
                    lang.f2(
                        "Enter a weight between 0 and {0} {1}.",
                        gym::fmt_load(units.weight_from_kg(gym::MAX_LOAD_KG).floor(), lang),
                        units.weight_unit(),
                    ),
                );
                None
            }
            Err(e) => {
                err('w', e.for_field(lang, lang.t("Weight")));
                None
            }
        };
        let (mut reps, mut duration_s) = (None, None);
        if is_timed {
            let typed = fields.get(&format!("d{}", s.id)).map_or("", String::as_str);
            match gym::parse_seconds(typed, gym::MAX_DURATION_S) {
                Ok(Some(d)) => duration_s = Some(d),
                Ok(None) if done => err(
                    'd',
                    lang.t("Enter the time you held, e.g. 45 or 1:30.").into(),
                ),
                Ok(None) => {}
                Err(ParseError::Invalid | ParseError::OutOfRange) => err(
                    'd',
                    lang.t("Enter a time up to 60 minutes, e.g. 45 or 1:30.")
                        .into(),
                ),
            }
        } else {
            let typed = fields.get(&format!("r{}", s.id)).map_or("", |v| v.trim());
            match typed.parse::<i64>() {
                Ok(r) if (0..=workouts::MAX_SET_REPS).contains(&r) => reps = Some(r),
                _ if typed.is_empty() => {
                    if done {
                        err('r', lang.t("Enter the reps you did.").into());
                    }
                }
                _ => err(
                    'r',
                    lang.f1(
                        "Enter a whole number of reps up to {0}.",
                        workouts::MAX_SET_REPS,
                    ),
                ),
            }
        }
        values.push(SetValues {
            id: s.id,
            reps,
            duration_s,
            weight_kg,
            done,
        });
    }
    (values, errors)
}

/// Checks the date, name and note of a workout.
fn read_details(
    t: &Typed,
    today: jiff::civil::Date,
    lang: Lang,
) -> Result<(String, String, Option<String>), FieldErrors> {
    let mut errors = FieldErrors::default();
    let date = match parse_date(&t.date) {
        Some(d) if d > today => {
            errors.add("date", lang.t("Choose a date that is not in the future."));
            None
        }
        Some(d) => Some(d.to_string()),
        None => {
            errors.add("date", lang.t("Choose a valid date."));
            None
        }
    };
    let name = match clean_text(&t.name, gym::MAX_NAME_CHARS) {
        Ok(Some(n)) => Some(n),
        Ok(None) => {
            errors.add("name", lang.t("Enter a name."));
            None
        }
        Err(e) => {
            errors.add("name", e.for_field(lang, lang.t("Name")));
            None
        }
    };
    let note = match clean_text(&t.note, workouts::MAX_NOTE_CHARS) {
        Ok(n) => n,
        Err(e) => {
            errors.add("note", e.for_field(lang, lang.t("Note")));
            None
        }
    };
    match (date, name) {
        (Some(d), Some(n)) if errors.is_empty() => Ok((d, n, note)),
        _ => Err(errors),
    }
}

/// "60 × 10", "+10 × 8", "45 s"; the unit is in the column header.
fn set_short(d: &Done, timed: bool, units: UnitSystem, lang: Lang) -> String {
    let load = d
        .weight_kg
        .filter(|kg| *kg > 0.0)
        .map(|kg| fmt_load_in(units, kg, lang));
    let amount = if timed {
        d.duration_s.map(gym::fmt_seconds)
    } else {
        d.reps.map(|r| r.to_string())
    };
    match (load, amount) {
        (Some(l), Some(a)) if timed => format!("{l} · {a}"),
        (Some(l), Some(a)) => format!("{l} × {a}"),
        (None, Some(a)) => a,
        (Some(l), None) => l,
        (None, None) => "–".into(),
    }
}

/// "60 kg × 10", "10 reps", "+10 kg × 8", "45 s".
fn set_line(d: &Done, timed: bool, bodyweight: bool, units: UnitSystem, lang: Lang) -> String {
    let load = d.weight_kg.filter(|kg| *kg > 0.0).map(|kg| {
        let l = format!("{} {}", fmt_load_in(units, kg, lang), units.weight_unit());
        if bodyweight { format!("+{l}") } else { l }
    });
    let amount = if timed {
        d.duration_s.map(gym::fmt_seconds)
    } else {
        d.reps.map(|r| {
            if r == 1 {
                lang.t("1 rep").to_string()
            } else {
                lang.f1("{0} reps", r)
            }
        })
    };
    match (load, amount) {
        (Some(l), Some(a)) if timed => format!("{l} · {a}"),
        (Some(l), Some(_)) => format!("{l} × {}", d.reps.unwrap_or(0)),
        (None, Some(a)) => a,
        (Some(l), None) => l,
        (None, None) => "–".into(),
    }
}

/// "3 × 8–12 reps · 60 kg · Rest 2 min".
fn plan_line(e: &WorkoutExercise, bodyweight: bool, units: UnitSystem, lang: Lang) -> String {
    let mut parts = Vec::new();
    if let Some(p) = e.plan() {
        let target = match (p.duration_s, p.reps_min) {
            (Some(d), _) => Some(Target::Duration(d)),
            (None, Some(min)) => Some(Target::Reps {
                min,
                max: p.reps_max,
            }),
            _ => None,
        };
        parts.push(match target {
            Some(t) => format!("{} × {}", p.sets, t.label(lang)),
            None => lang.f1("{0} sets", p.sets),
        });
        if let Some(kg) = p.weight_kg {
            let l = format!("{} {}", fmt_load_in(units, kg, lang), units.weight_unit());
            parts.push(if bodyweight { format!("+{l}") } else { l });
        }
    }
    if let Some(r) = e.rest_s.filter(|r| *r > 0) {
        parts.push(lang.f1("Rest {0}", gym::fmt_seconds(r)));
    }
    parts.join(" · ")
}

fn done_of(s: &WorkoutSet) -> Done {
    Done {
        reps: s.reps,
        duration_s: s.duration_s,
        weight_kg: s.weight_kg,
    }
}

/// Whether a workout exercise is done with body weight (its load is added
/// weight).
async fn bodyweight_of(
    state: &AppState,
    list: &[WorkoutExercise],
) -> AppResult<HashMap<i64, bool>> {
    let needs_custom = list.iter().any(|e| e.custom_exercise_id.is_some());
    let customs = if needs_custom {
        gym::custom_exercises(&state.db).await?
    } else {
        Vec::new()
    };
    Ok(list
        .iter()
        .map(|e| {
            let equipment = match (&e.exercise_key, e.custom_exercise_id) {
                (Some(k), _) => exercises::find(k).map(|x| x.equipment),
                (None, Some(c)) => customs
                    .iter()
                    .find(|x| x.id == c)
                    .and_then(|x| Equipment::parse(&x.equipment)),
                _ => None,
            };
            (e.id, equipment == Some(Equipment::Bodyweight))
        })
        .collect())
}

async fn workout_page(
    state: &AppState,
    w: &Workout,
    typed: &Typed,
    errors: FieldErrors,
    set_errors: Vec<SetError>,
    message: Option<&'static str>,
) -> AppResult<WorkoutPage> {
    let lang = current_lang();
    let units = profile::load(&state.db).await?.units();
    let list = workouts::exercises_of(&state.db, w.id).await?;
    let sets = workouts::sets_of(&state.db, w.id).await?;
    let bodyweight = bodyweight_of(state, &list).await?;
    let keys: Vec<ExerciseKey> = list.iter().filter_map(WorkoutExercise::key).collect();
    let last = workouts::last_done(&state.db, &keys, Some((&w.workout_date, w.id))).await?;
    let err_of = |id: i64, field: char| {
        set_errors
            .iter()
            .find(|e| e.set_id == id && e.field == field)
            .map(|e| e.message.clone())
    };
    let typed_or =
        |name: String, stored: String| typed.fields.get(&name).cloned().unwrap_or(stored);
    let mut summary: Vec<(String, String)> = errors
        .iter()
        .map(|(f, m)| (f.to_string(), m.clone()))
        .collect();
    let mut blocks = Vec::new();
    for e in &list {
        let prev = e
            .key()
            .and_then(|k| last.get(&k))
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let name = e.name(lang);
        let bw = bodyweight.get(&e.id).copied().unwrap_or(false);
        let mut rows = Vec::new();
        for (i, s) in sets
            .iter()
            .filter(|s| s.workout_exercise_id == e.id)
            .enumerate()
        {
            let has_typed = typed.fields.contains_key(&format!("w{}", s.id));
            let row = SetRow {
                id: s.id,
                number: i + 1,
                previous: prev
                    .get(i)
                    .map(|d| set_short(d, e.timed, units, lang))
                    .unwrap_or_default(),
                weight: typed_or(
                    format!("w{}", s.id),
                    s.weight_kg
                        .map(|kg| fmt_load_in(units, kg, lang))
                        .unwrap_or_default(),
                ),
                reps: typed_or(
                    format!("r{}", s.id),
                    s.reps.map(|r| r.to_string()).unwrap_or_default(),
                ),
                duration: typed_or(
                    format!("d{}", s.id),
                    s.duration_s.map(gym::seconds_input).unwrap_or_default(),
                ),
                done: if has_typed {
                    typed.fields.contains_key(&format!("done{}", s.id))
                } else {
                    s.done
                },
                weight_error: err_of(s.id, 'w'),
                reps_error: err_of(s.id, 'r'),
                duration_error: err_of(s.id, 'd'),
            };
            for (field, msg) in [
                ('w', &row.weight_error),
                ('r', &row.reps_error),
                ('d', &row.duration_error),
            ] {
                if let Some(m) = msg {
                    summary.push((
                        format!("{field}{}", s.id),
                        lang.f("{0}, set {1}: {2}", &[&name, &row.number, m]),
                    ));
                }
            }
            rows.push(row);
        }
        blocks.push(ExerciseBlock {
            id: e.id,
            timed: e.timed,
            bodyweight: bw,
            plan: plan_line(e, bw, units, lang),
            progress: w.is_open() && !e.timed && workouts::reached_top(e.plan().as_ref(), prev),
            rest_s: e.rest_s.unwrap_or(0),
            can_add_set: (rows.len() as i64) < gym::MAX_SETS,
            sets: rows,
            name,
        });
    }
    Ok(WorkoutPage {
        lang,
        title: w.name.clone(),
        nav: NAV,
        message,
        id: w.id,
        open: w.is_open(),
        action: format!("/gym/workouts/{}", w.id),
        cancel_url: format!("/gym/workouts/{}", w.id),
        unit: units.weight_unit(),
        date: typed.date.clone(),
        name: typed.name.clone(),
        note: typed.note.clone(),
        errors,
        summary,
        can_add_exercise: w.is_open() && (blocks.len() as i64) < workouts::MAX_WORKOUT_EXERCISES,
        exercises: blocks,
    })
}

// ---------- Viewing ----------

pub struct DoneExercise {
    pub name: String,
    pub sets: Vec<String>,
}

#[derive(Template)]
#[template(path = "gym_workout_summary.html")]
pub struct SummaryPage {
    pub lang: Lang,
    pub title: String,
    pub nav: &'static str,
    pub message: Option<&'static str>,
    pub id: i64,
    pub subtitle: String,
    pub stats: Vec<(&'static str, String)>,
    pub exercises: Vec<DoneExercise>,
    pub note: String,
}

/// A total load in whole units with digit grouping: "12,160 kg".
fn fmt_volume(units: UnitSystem, kg: f64, lang: Lang) -> String {
    let whole = units.weight_from_kg(kg).round() as i64;
    format!("{} {}", fmt_count(whole, lang), units.weight_unit())
}

fn fmt_minutes(min: i64) -> String {
    match (min / 60, min % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m:02}"),
    }
}

pub async fn detail(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<MsgQuery>,
) -> AppResult<Response> {
    let lang = current_lang();
    let w = load(&state, &id).await?;
    let message = flash_message(q.msg.as_deref(), lang);
    if w.is_open() {
        let page = workout_page(
            &state,
            &w,
            &Typed::of(&w),
            FieldErrors::default(),
            Vec::new(),
            message,
        )
        .await?;
        return Ok(render(&page)?.into_response());
    }
    let units = profile::load(&state.db).await?.units();
    let list = workouts::exercises_of(&state.db, w.id).await?;
    let sets = workouts::sets_of(&state.db, w.id).await?;
    let bodyweight = bodyweight_of(&state, &list).await?;
    let done: Vec<Done> = sets.iter().map(done_of).collect();
    let exercises = list
        .iter()
        .map(|e| {
            let bw = bodyweight.get(&e.id).copied().unwrap_or(false);
            DoneExercise {
                name: e.name(lang),
                sets: sets
                    .iter()
                    .filter(|s| s.workout_exercise_id == e.id)
                    .map(|s| set_line(&done_of(s), e.timed, bw, units, lang))
                    .collect(),
            }
        })
        .collect();
    let mut stats = vec![
        (lang.t("Exercises"), list.len().to_string()),
        (lang.t("Sets"), sets.len().to_string()),
    ];
    let volume = workouts::volume_kg(&done);
    if volume > 0.0 {
        stats.push((lang.t("Volume"), fmt_volume(units, volume, lang)));
    }
    if let Some(min) = w.duration_min() {
        stats.push((lang.t("Duration"), fmt_minutes(min)));
    }
    let page = SummaryPage {
        lang,
        title: w.name.clone(),
        nav: NAV,
        message,
        id: w.id,
        subtitle: parse_date(&w.workout_date)
            .map_or_else(|| w.workout_date.clone(), |d| fmt_date_long(d, lang)),
        stats,
        exercises,
        note: w.note.clone().unwrap_or_default(),
    };
    Ok(render(&page)?.into_response())
}

pub async fn edit_form(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<Response> {
    let w = load(&state, &id).await?;
    if w.is_open() {
        return Ok(see_other(&format!("/gym/workouts/{}", w.id)));
    }
    let page = workout_page(
        &state,
        &w,
        &Typed::of(&w),
        FieldErrors::default(),
        Vec::new(),
        None,
    )
    .await?;
    Ok(render(&page)?.into_response())
}

/// What a submit button of the workout form asks for, after saving.
#[derive(Debug, PartialEq)]
enum Action {
    Save,
    Finish,
    AddExercise,
    AddSet(i64),
    RemoveSet(i64),
    RemoveExercise(i64),
}

impl Action {
    fn parse(s: &str) -> Option<Action> {
        let (verb, id) = s.split_once(':').unwrap_or((s, ""));
        let id = || id.parse::<i64>().ok().filter(|i| *i > 0);
        Some(match verb {
            "" | "save" => Action::Save,
            "finish" => Action::Finish,
            "add_exercise" => Action::AddExercise,
            "add_set" => Action::AddSet(id()?),
            "remove_set" => Action::RemoveSet(id()?),
            "remove_exercise" => Action::RemoveExercise(id()?),
            _ => return None,
        })
    }

    fn exercise(&self) -> Option<i64> {
        match self {
            Action::AddSet(e) | Action::RemoveSet(e) | Action::RemoveExercise(e) => Some(*e),
            _ => None,
        }
    }
}

pub async fn save(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Form(pairs): Form<Vec<(String, String)>>,
) -> AppResult<Response> {
    let lang = current_lang();
    let w = load(&state, &id).await?;
    let autosave = headers.contains_key(AUTOSAVE_HEADER);
    let (typed, action) = Typed::from_form(pairs);
    let action = Action::parse(&action)
        .ok_or_else(|| AppError::BadRequest(lang.t("Unknown action.").into()))?;
    let profile = profile::load(&state.db).await?;
    let units = profile.units();
    let list = workouts::exercises_of(&state.db, w.id).await?;
    if let Some(eid) = action.exercise()
        && !list.iter().any(|e| e.id == eid)
    {
        return Err(AppError::NotFound);
    }
    let sets = workouts::sets_of(&state.db, w.id).await?;
    let timed: HashMap<i64, bool> = list.iter().map(|e| (e.id, e.timed)).collect();
    let (values, set_errors) = read_sets(&typed.fields, &sets, &timed, !w.is_open(), units, lang);
    let details = read_details(&typed, today_in(&profile.timezone), lang);
    let (date, name, note) = match details {
        Ok(d) if set_errors.is_empty() => d,
        result => {
            if autosave {
                return Ok(StatusCode::UNPROCESSABLE_ENTITY.into_response());
            }
            let errors = result.err().unwrap_or_default();
            let page = workout_page(&state, &w, &typed, errors, set_errors, None).await?;
            return render_status(StatusCode::UNPROCESSABLE_ENTITY, &page);
        }
    };
    let details = Details {
        date: &date,
        name: &name,
        note: note.as_deref(),
    };
    workouts::save(&state.db, w.id, &details, &values).await?;
    if autosave {
        return Ok(StatusCode::NO_CONTENT.into_response());
    }
    let here = workout_url(&w);
    Ok(see_other(&match action {
        Action::Save if w.is_open() => format!("{here}?msg=workout_saved"),
        Action::Save => format!("/gym/workouts/{}?msg=workout_saved", w.id),
        Action::Finish if !w.is_open() => format!("/gym/workouts/{}", w.id),
        Action::Finish => match workouts::finish(&state.db, w.id).await? {
            Finished::Finished => format!("/gym/workouts/{}?msg=workout_finished", w.id),
            Finished::NothingDone => {
                let mut errors = FieldErrors::default();
                errors.add(
                    "finish",
                    lang.t("Mark at least one set as done to finish the workout, or discard it."),
                );
                let w = load(&state, &id).await?;
                let page =
                    workout_page(&state, &w, &Typed::of(&w), errors, Vec::new(), None).await?;
                return render_status(StatusCode::UNPROCESSABLE_ENTITY, &page);
            }
        },
        Action::AddExercise if w.is_open() => exercises_url(PickFor::Workout(w.id), "", None),
        Action::AddExercise => here,
        Action::AddSet(e) => {
            workouts::add_set(&state.db, e, !w.is_open()).await?;
            format!("{here}#exercise-{e}")
        }
        Action::RemoveSet(e) => {
            workouts::remove_last_set(&state.db, e).await?;
            format!("{here}#exercise-{e}")
        }
        Action::RemoveExercise(e) => {
            workouts::remove_exercise(&state.db, e).await?;
            here
        }
    }))
}

pub async fn delete(State(state): State<AppState>, Path(id): Path<String>) -> AppResult<Response> {
    let w = load(&state, &id).await?;
    workouts::delete(&state.db, w.id).await?;
    Ok(see_other(if w.is_open() {
        "/gym?msg=workout_discarded"
    } else {
        "/gym/workouts?msg=workout_deleted"
    }))
}

// ---------- History ----------

pub struct HistoryRow {
    pub url: String,
    pub name: String,
    pub sub: String,
}

/// A finished workout as a list row: "22 Oct 2026 · 5 exercises · 18 sets".
pub(super) fn history_row(s: &workouts::WorkoutSummary, lang: Lang) -> HistoryRow {
    let w = &s.workout;
    let date = parse_date(&w.workout_date)
        .map_or_else(|| w.workout_date.clone(), |d| fmt_date_short(d, lang));
    let exercises = match s.exercises {
        1 => lang.t("1 exercise").to_string(),
        n => lang.f1("{0} exercises", n),
    };
    let sets = match s.sets {
        1 => lang.t("1 set").to_string(),
        n => lang.f1("{0} sets", n),
    };
    HistoryRow {
        url: format!("/gym/workouts/{}", w.id),
        name: w.name.clone(),
        sub: format!("{date} · {exercises} · {sets}"),
    }
}

#[derive(Deserialize)]
pub struct HistoryQuery {
    page: Option<i64>,
    msg: Option<String>,
}

#[derive(Template)]
#[template(path = "gym_workouts.html")]
pub struct HistoryPage {
    pub lang: Lang,
    pub title: String,
    pub nav: &'static str,
    pub message: Option<&'static str>,
    pub rows: Vec<HistoryRow>,
    pub newer_url: Option<String>,
    pub older_url: Option<String>,
}

pub async fn history(
    State(state): State<AppState>,
    Query(q): Query<HistoryQuery>,
) -> AppResult<Response> {
    let lang = current_lang();
    let total = workouts::finished_count(&state.db).await?;
    let pages = ((total + PER_PAGE - 1) / PER_PAGE).max(1);
    let page = q.page.unwrap_or(1).clamp(1, pages);
    let rows = workouts::history(&state.db, PER_PAGE, (page - 1) * PER_PAGE)
        .await?
        .iter()
        .map(|s| history_row(s, lang))
        .collect();
    let url = |p: i64| format!("/gym/workouts?page={p}");
    let page = HistoryPage {
        lang,
        title: lang.t("Workout history").into(),
        nav: NAV,
        message: flash_message(q.msg.as_deref(), lang),
        rows,
        newer_url: (page > 1).then(|| url(page - 1)),
        older_url: (page < pages).then(|| url(page + 1)),
    };
    Ok(render(&page)?.into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(id: i64, exercise: i64) -> WorkoutSet {
        WorkoutSet {
            id,
            workout_exercise_id: exercise,
            position: 0,
            reps: None,
            duration_s: None,
            weight_kg: None,
            done: false,
        }
    }

    fn fields(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn typed_sets_are_read_in_the_profile_units() {
        let sets = [set(1, 10), set(2, 10), set(3, 20)];
        let timed = HashMap::from([(10, false), (20, true)]);
        let f = fields(&[
            ("w1", "135"),
            ("r1", "8"),
            ("done1", "on"),
            ("w2", ""),
            ("r2", ""),
            ("w3", ""),
            ("d3", "1:15"),
            ("done3", "on"),
        ]);
        let (values, errors) = read_sets(&f, &sets, &timed, false, UnitSystem::Imperial, Lang::En);
        assert!(errors.is_empty(), "{errors:?}");
        assert!((values[0].weight_kg.unwrap() - 61.234970).abs() < 1e-5);
        assert_eq!((values[0].reps, values[0].done), (Some(8), true));
        assert_eq!(
            values[1],
            SetValues {
                id: 2,
                reps: None,
                duration_s: None,
                weight_kg: None,
                done: false
            },
            "an empty set left undone is fine"
        );
        assert_eq!((values[2].duration_s, values[2].reps), (Some(75), None));
    }

    #[test]
    fn done_sets_need_their_reps_or_time() {
        let sets = [set(1, 10), set(2, 20), set(3, 10)];
        let timed = HashMap::from([(10, false), (20, true)]);
        let f = fields(&[
            ("w1", "60"),
            ("r1", ""),
            ("done1", "on"),
            ("w2", ""),
            ("d2", ""),
            ("done2", "on"),
            ("w3", "-1"),
            ("r3", "8.5"),
        ]);
        let (_, errors) = read_sets(&f, &sets, &timed, false, UnitSystem::Metric, Lang::En);
        let got: Vec<(i64, char)> = errors.iter().map(|e| (e.set_id, e.field)).collect();
        assert_eq!(got, [(1, 'r'), (2, 'd'), (3, 'w'), (3, 'r')]);
    }

    #[test]
    fn sets_missing_from_the_form_are_left_alone_and_finished_sets_stay_done() {
        let sets = [set(1, 10), set(2, 10)];
        let timed = HashMap::from([(10, false)]);
        let (values, errors) = read_sets(
            &fields(&[("w1", "50"), ("r1", "5")]),
            &sets,
            &timed,
            true,
            UnitSystem::Metric,
            Lang::En,
        );
        assert!(errors.is_empty());
        assert_eq!(values.len(), 1);
        assert!(values[0].done);
    }

    #[test]
    fn actions_name_their_exercise() {
        assert_eq!(Action::parse(""), Some(Action::Save));
        assert_eq!(Action::parse("finish"), Some(Action::Finish));
        assert_eq!(Action::parse("add_set:4"), Some(Action::AddSet(4)));
        assert_eq!(
            Action::parse("remove_exercise:9"),
            Some(Action::RemoveExercise(9))
        );
        assert_eq!(Action::parse("add_set:"), None);
        assert_eq!(Action::parse("add_set:-1"), None);
        assert_eq!(Action::parse("drop_table"), None);
    }

    #[test]
    fn details_refuse_future_dates_and_empty_names() {
        let today = jiff::civil::date(2026, 10, 24);
        let t = |date: &str, name: &str| Typed {
            date: date.into(),
            name: name.into(),
            ..Default::default()
        };
        assert!(read_details(&t("2026-10-24", "Push"), today, Lang::En).is_ok());
        assert!(
            read_details(&t("2026-10-25", "Push"), today, Lang::En)
                .unwrap_err()
                .has("date")
        );
        assert!(
            read_details(&t("2026-10-20", " "), today, Lang::En)
                .unwrap_err()
                .has("name")
        );
    }

    #[test]
    fn sets_read_naturally() {
        let d = |reps, kg| Done {
            reps,
            duration_s: None,
            weight_kg: kg,
        };
        let m = UnitSystem::Metric;
        assert_eq!(
            set_line(&d(Some(10), Some(60.0)), false, false, m, Lang::En),
            "60 kg × 10"
        );
        assert_eq!(
            set_line(&d(Some(8), Some(10.0)), false, true, m, Lang::En),
            "+10 kg × 8"
        );
        assert_eq!(
            set_line(&d(Some(12), None), false, true, m, Lang::En),
            "12 reps"
        );
        assert_eq!(
            set_line(&d(Some(1), Some(0.0)), false, false, m, Lang::Fr),
            "1 répétition"
        );
        assert_eq!(
            set_short(&d(Some(10), Some(62.5)), false, m, Lang::Fr),
            "62,5 × 10"
        );
        let held = Done {
            reps: None,
            duration_s: Some(45),
            weight_kg: None,
        };
        assert_eq!(set_short(&held, true, m, Lang::En), "45 s");
        assert_eq!(fmt_minutes(65), "1 h 05");
    }
}
