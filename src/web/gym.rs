//! Gym programs: overview, programs, routines, the exercise list and the
//! plan of each exercise in a routine. Workouts are in `gym_workouts`.

use askama::Template;
use axum::Form;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use super::activity::{DayOption, MenuOption};
use super::gym_workouts::{HistoryRow, history_row};
use super::{AppState, current_lang, flash_message, parse_id, render, render_status, see_other};
use crate::activity::Weekdays;
use crate::error::{AppError, AppResult};
use crate::foods::normalize;
use crate::forms::FieldErrors;
use crate::gym::exercises::{self, Equipment, Muscle};
use crate::gym::starter::{self, STARTERS};
use crate::gym::workouts;
use crate::gym::{
    self, CustomInput, DeleteResult, ExerciseRef, ItemInput, Move, ParseError, Program,
    RoutineExercise, Schedule, Target,
};
use crate::i18n::Lang;
use crate::profile;
use crate::units::{UnitSystem, clean_text, parse_decimal, today_in, weekday_long, weekday_short};

pub(super) const NAV: &str = "activity";

#[derive(Deserialize)]
pub struct MsgQuery {
    msg: Option<String>,
}

fn name_field(input: &str, errors: &mut FieldErrors, lang: Lang) -> Option<String> {
    match clean_text(input, gym::MAX_NAME_CHARS) {
        Ok(Some(n)) => Some(n),
        Ok(None) => {
            errors.add("name", lang.t("Enter a name."));
            None
        }
        Err(e) => {
            errors.add("name", e.for_field(lang, lang.t("Name")));
            None
        }
    }
}

/// "6 exercises", "1 exercise".
fn exercise_count(n: i64, lang: Lang) -> String {
    match n {
        0 => lang.t("No exercises").into(),
        1 => lang.t("1 exercise").into(),
        _ => lang.f1("{0} exercises", n),
    }
}

fn routine_count(n: i64, lang: Lang) -> String {
    match n {
        0 => lang.t("No routines").into(),
        1 => lang.t("1 routine").into(),
        _ => lang.f1("{0} routines", n),
    }
}

/// "Mon, Thu", or "No day chosen".
fn days_label(days: Weekdays, lang: Lang) -> String {
    if days.is_empty() {
        return lang.t("No day chosen").into();
    }
    if days == Weekdays::ALL {
        return lang.t("Every day").into();
    }
    (0..7)
        .filter(|i| days.has(*i))
        .map(|i| weekday_short(i, lang))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The seven weekday toggles of a routine form.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct DayFields {
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

impl DayFields {
    fn days(&self) -> Weekdays {
        [
            &self.day0, &self.day1, &self.day2, &self.day3, &self.day4, &self.day5, &self.day6,
        ]
        .into_iter()
        .enumerate()
        .filter(|(_, v)| !v.is_empty())
        .fold(Weekdays::NONE, |w, (i, _)| w.with(i))
    }

    fn of(days: Weekdays) -> Self {
        let on = |i| {
            if days.has(i) {
                "on".into()
            } else {
                String::new()
            }
        };
        DayFields {
            day0: on(0),
            day1: on(1),
            day2: on(2),
            day3: on(3),
            day4: on(4),
            day5: on(5),
            day6: on(6),
        }
    }

    fn options(&self, lang: Lang) -> Vec<DayOption> {
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

// ---------- Overview ----------

pub struct RoutineLink {
    pub url: String,
    pub name: String,
    pub sub: String,
}

pub struct DayRow {
    pub name: &'static str,
    pub today: bool,
    pub routines: Vec<RoutineLink>,
}

pub struct ProgramRow {
    pub url: String,
    pub name: String,
    pub sub: String,
}

pub struct StarterOption {
    pub key: &'static str,
    pub name: &'static str,
    pub description: &'static str,
}

fn starter_options(lang: Lang) -> Vec<StarterOption> {
    STARTERS
        .iter()
        .map(|s| StarterOption {
            key: s.key,
            name: s.name(lang),
            description: s.description(lang),
        })
        .collect()
}

#[derive(Template)]
#[template(path = "gym.html")]
pub struct GymPage {
    pub lang: Lang,
    pub title: String,
    pub nav: &'static str,
    pub message: Option<&'static str>,
    pub active: Option<Program>,
    pub active_url: String,
    pub weekly: bool,
    /// Weekly programs: the seven days, Monday first.
    pub days: Vec<DayRow>,
    /// Rotation programs, and routines of a weekly program on no day.
    pub sequence: Vec<RoutineLink>,
    pub others: Vec<ProgramRow>,
    pub starters: Vec<StarterOption>,
    /// The workout in progress.
    pub open_workout: Option<RoutineLink>,
    /// Routines to start a workout from: today's, or the next of a rotation.
    pub suggested: Vec<Suggestion>,
    pub recent: Vec<HistoryRow>,
    pub more_history: bool,
}

pub struct Suggestion {
    pub routine_id: i64,
    pub name: String,
    pub sub: String,
}

/// Finished workouts shown on the Gym page.
const RECENT_WORKOUTS: i64 = 3;

pub async fn page(State(state): State<AppState>, Query(q): Query<MsgQuery>) -> AppResult<Response> {
    let lang = current_lang();
    let all = gym::programs(&state.db).await?;
    let active = all
        .iter()
        .find(|p| p.program.active)
        .map(|p| p.program.clone());
    let others = all
        .iter()
        .filter(|p| !p.program.active)
        .map(|p| ProgramRow {
            url: format!("/gym/programs/{}", p.program.id),
            name: p.program.name.clone(),
            sub: format!(
                "{} · {}",
                p.program.schedule().label(lang),
                routine_count(p.routines, lang)
            ),
        })
        .collect();
    let mut days = Vec::new();
    let mut sequence = Vec::new();
    let mut suggested = Vec::new();
    let (mut weekly, mut active_url) = (false, String::new());
    let open = workouts::open(&state.db).await?;
    let open_workout = match &open {
        Some(w) => Some(RoutineLink {
            url: format!("/gym/workouts/{}", w.id),
            name: w.name.clone(),
            sub: format!(
                "{} · {}",
                lang.t("In progress"),
                exercise_count(workouts::exercise_count(&state.db, w.id).await?, lang)
            ),
        }),
        None => None,
    };
    if let Some(p) = &active {
        let tz = profile::load(&state.db).await?.timezone;
        let today = Weekdays::index_of(today_in(&tz));
        let routines = gym::routines(&state.db, p.id).await?;
        let link = |r: &gym::RoutineSummary| RoutineLink {
            url: format!("/gym/routines/{}", r.routine.id),
            name: r.routine.name.clone(),
            sub: exercise_count(r.exercises, lang),
        };
        weekly = p.schedule() == Schedule::Weekly;
        active_url = format!("/gym/programs/{}", p.id);
        let suggest = |r: &gym::RoutineSummary, why: &str| Suggestion {
            routine_id: r.routine.id,
            name: r.routine.name.clone(),
            sub: format!("{why} · {}", exercise_count(r.exercises, lang)),
        };
        if open.is_none() {
            if weekly {
                suggested = routines
                    .iter()
                    .filter(|r| r.routine.days().has(today) && r.exercises > 0)
                    .map(|r| suggest(r, lang.t("Today")))
                    .collect();
            } else if !routines.is_empty() {
                let last = workouts::last_routine_of_program(&state.db, p.id).await?;
                let next = last
                    .and_then(|id| routines.iter().position(|r| r.routine.id == id))
                    .map_or(0, |i| (i + 1) % routines.len());
                if routines[next].exercises > 0 {
                    suggested.push(suggest(&routines[next], lang.t("Next in the rotation")));
                }
            }
        }
        if weekly {
            days = (0..7)
                .map(|i| DayRow {
                    name: weekday_long(i, lang),
                    today: i == today,
                    routines: routines
                        .iter()
                        .filter(|r| r.routine.days().has(i))
                        .map(link)
                        .collect(),
                })
                .collect();
            sequence = routines
                .iter()
                .filter(|r| r.routine.days().is_empty())
                .map(link)
                .collect();
        } else {
            sequence = routines.iter().map(link).collect();
        }
    }
    let page = GymPage {
        lang,
        title: lang.t("Gym").into(),
        nav: NAV,
        message: flash_message(q.msg.as_deref(), lang),
        active,
        active_url,
        weekly,
        days,
        sequence,
        others,
        starters: starter_options(lang),
        open_workout,
        suggested,
        recent: workouts::history(&state.db, RECENT_WORKOUTS, 0)
            .await?
            .iter()
            .map(|s| history_row(s, lang))
            .collect(),
        more_history: workouts::finished_count(&state.db).await? > RECENT_WORKOUTS,
    };
    Ok(render(&page)?.into_response())
}

// ---------- Programs ----------

#[derive(Clone, Debug, Default, Deserialize)]
pub struct ProgramForm {
    #[serde(default)]
    name: String,
    #[serde(default)]
    schedule: String,
    /// A starter program to copy instead of starting empty.
    #[serde(default)]
    starter: String,
}

pub struct ScheduleOption {
    pub value: &'static str,
    pub label: &'static str,
    pub checked: bool,
}

fn schedule_options(selected: &str, lang: Lang) -> Vec<ScheduleOption> {
    [Schedule::Weekly, Schedule::Rotation]
        .into_iter()
        .map(|s| ScheduleOption {
            value: s.as_str(),
            label: s.label(lang),
            checked: selected == s.as_str(),
        })
        .collect()
}

#[derive(Template)]
#[template(path = "gym_program_form.html")]
pub struct ProgramFormPage {
    pub lang: Lang,
    pub title: String,
    pub nav: &'static str,
    /// The program being edited; `None` when creating one.
    pub id: Option<i64>,
    pub back_url: String,
    pub name: String,
    pub schedules: Vec<ScheduleOption>,
    pub starters: Vec<StarterOption>,
    pub errors: FieldErrors,
}

fn program_form_page(id: Option<i64>, form: &ProgramForm, errors: FieldErrors) -> ProgramFormPage {
    let lang = current_lang();
    ProgramFormPage {
        lang,
        title: if id.is_some() {
            lang.t("Edit program").into()
        } else {
            lang.t("New program").into()
        },
        nav: NAV,
        id,
        back_url: id.map_or("/gym".into(), |id| format!("/gym/programs/{id}")),
        name: form.name.clone(),
        schedules: schedule_options(&form.schedule, lang),
        starters: if id.is_some() {
            Vec::new()
        } else {
            starter_options(lang)
        },
        errors,
    }
}

pub async fn new_program_form() -> AppResult<Response> {
    let form = ProgramForm {
        schedule: Schedule::Weekly.as_str().into(),
        ..Default::default()
    };
    Ok(render(&program_form_page(None, &form, FieldErrors::default()))?.into_response())
}

fn validate_program(form: &ProgramForm, lang: Lang) -> Result<(String, Schedule), FieldErrors> {
    let mut errors = FieldErrors::default();
    let name = name_field(&form.name, &mut errors, lang);
    let schedule = Schedule::parse(&form.schedule);
    if schedule.is_none() {
        errors.add("schedule", lang.t("Choose how the routines are scheduled."));
    }
    match (name, schedule) {
        (Some(n), Some(s)) if errors.is_empty() => Ok((n, s)),
        _ => Err(errors),
    }
}

async fn check_program_room(state: &AppState, lang: Lang) -> AppResult<()> {
    if gym::program_count(&state.db).await? >= gym::MAX_PROGRAMS {
        return Err(AppError::BadRequest(lang.f1(
            "You can keep up to {0} programs. Delete one you no longer use first.",
            gym::MAX_PROGRAMS,
        )));
    }
    Ok(())
}

pub async fn create_program(
    State(state): State<AppState>,
    Form(form): Form<ProgramForm>,
) -> AppResult<Response> {
    let lang = current_lang();
    check_program_room(&state, lang).await?;
    if !form.starter.is_empty() {
        let s = starter::find(&form.starter).ok_or(AppError::NotFound)?;
        let id = starter::create(&state.db, s, s.name(lang), lang).await?;
        return Ok(see_other(&format!(
            "/gym/programs/{id}?msg=program_created"
        )));
    }
    match validate_program(&form, lang) {
        Ok((name, schedule)) => {
            let id = gym::create_program(&state.db, &name, schedule).await?;
            Ok(see_other(&format!(
                "/gym/programs/{id}?msg=program_created"
            )))
        }
        Err(errors) => render_status(
            StatusCode::UNPROCESSABLE_ENTITY,
            &program_form_page(None, &form, errors),
        ),
    }
}

async fn load_program(state: &AppState, id: &str) -> AppResult<Program> {
    gym::program(&state.db, parse_id(id)?)
        .await?
        .ok_or(AppError::NotFound)
}

pub struct RoutineRow {
    pub id: i64,
    pub url: String,
    pub name: String,
    pub sub: String,
    pub first: bool,
    pub last: bool,
}

#[derive(Template)]
#[template(path = "gym_program.html")]
pub struct ProgramPage {
    pub lang: Lang,
    pub title: String,
    pub nav: &'static str,
    pub message: Option<&'static str>,
    pub program: Program,
    pub schedule_label: &'static str,
    pub weekly: bool,
    pub routines: Vec<RoutineRow>,
    pub can_add: bool,
    /// The add-routine form.
    pub name: String,
    pub days: Vec<DayOption>,
    pub errors: FieldErrors,
}

async fn program_page(
    state: &AppState,
    program: Program,
    message: Option<&'static str>,
    form: &RoutineForm,
    errors: FieldErrors,
) -> AppResult<ProgramPage> {
    let lang = current_lang();
    let summaries = gym::routines(&state.db, program.id).await?;
    let weekly = program.schedule() == Schedule::Weekly;
    let n = summaries.len();
    let routines = summaries
        .iter()
        .enumerate()
        .map(|(i, r)| RoutineRow {
            id: r.routine.id,
            url: format!("/gym/routines/{}", r.routine.id),
            name: r.routine.name.clone(),
            sub: if weekly {
                format!(
                    "{} · {}",
                    days_label(r.routine.days(), lang),
                    exercise_count(r.exercises, lang)
                )
            } else {
                exercise_count(r.exercises, lang)
            },
            first: i == 0,
            last: i + 1 == n,
        })
        .collect();
    Ok(ProgramPage {
        lang,
        title: program.name.clone(),
        nav: NAV,
        message,
        schedule_label: program.schedule().label(lang),
        weekly,
        routines,
        can_add: (n as i64) < gym::MAX_ROUTINES,
        name: form.name.clone(),
        days: form.days.options(lang),
        errors,
        program,
    })
}

pub async fn program_detail(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<MsgQuery>,
) -> AppResult<Response> {
    let program = load_program(&state, &id).await?;
    let message = flash_message(q.msg.as_deref(), current_lang());
    let page = program_page(
        &state,
        program,
        message,
        &RoutineForm::default(),
        FieldErrors::default(),
    )
    .await?;
    Ok(render(&page)?.into_response())
}

pub async fn edit_program_form(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<Response> {
    let p = load_program(&state, &id).await?;
    let form = ProgramForm {
        name: p.name.clone(),
        schedule: p.schedule.clone(),
        starter: String::new(),
    };
    Ok(render(&program_form_page(
        Some(p.id),
        &form,
        FieldErrors::default(),
    ))?
    .into_response())
}

pub async fn update_program(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Form(form): Form<ProgramForm>,
) -> AppResult<Response> {
    let p = load_program(&state, &id).await?;
    match validate_program(&form, current_lang()) {
        Ok((name, schedule)) => {
            if !gym::update_program(&state.db, p.id, &name, schedule).await? {
                return Err(AppError::NotFound);
            }
            Ok(see_other(&format!(
                "/gym/programs/{}?msg=program_saved",
                p.id
            )))
        }
        Err(errors) => render_status(
            StatusCode::UNPROCESSABLE_ENTITY,
            &program_form_page(Some(p.id), &form, errors),
        ),
    }
}

pub async fn activate_program(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<Response> {
    let id = parse_id(&id)?;
    if !gym::activate_program(&state.db, id).await? {
        return Err(AppError::NotFound);
    }
    Ok(see_other("/gym?msg=program_activated"))
}

pub async fn delete_program(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<Response> {
    let id = parse_id(&id)?;
    if !gym::delete_program(&state.db, id).await? {
        return Err(AppError::NotFound);
    }
    Ok(see_other("/gym?msg=program_deleted"))
}

// ---------- Routines ----------

#[derive(Clone, Debug, Default, Deserialize)]
pub struct RoutineForm {
    #[serde(default)]
    name: String,
    #[serde(flatten)]
    days: DayFields,
}

pub async fn create_routine(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Form(form): Form<RoutineForm>,
) -> AppResult<Response> {
    let lang = current_lang();
    let program = load_program(&state, &id).await?;
    if gym::routine_count(&state.db, program.id).await? >= gym::MAX_ROUTINES {
        return Err(AppError::BadRequest(
            lang.f1("A program can hold up to {0} routines.", gym::MAX_ROUTINES),
        ));
    }
    let mut errors = FieldErrors::default();
    match name_field(&form.name, &mut errors, lang) {
        Some(name) => {
            let days = match program.schedule() {
                Schedule::Weekly => form.days.days(),
                Schedule::Rotation => Weekdays::NONE,
            };
            let rid = gym::create_routine(&state.db, program.id, &name, days).await?;
            Ok(see_other(&format!(
                "/gym/routines/{rid}?msg=routine_created"
            )))
        }
        None => {
            let page = program_page(&state, program, None, &form, errors).await?;
            render_status(StatusCode::UNPROCESSABLE_ENTITY, &page)
        }
    }
}

pub(super) async fn load_routine(state: &AppState, id: &str) -> AppResult<(gym::Routine, Program)> {
    let r = gym::routine(&state.db, parse_id(id)?)
        .await?
        .ok_or(AppError::NotFound)?;
    let p = gym::program(&state.db, r.program_id)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok((r, p))
}

pub struct ItemRow {
    pub id: i64,
    pub url: String,
    pub name: String,
    pub plan: String,
    pub detail: String,
    pub note: String,
    pub first: bool,
    pub last: bool,
}

#[derive(Template)]
#[template(path = "gym_routine.html")]
pub struct RoutinePage {
    pub lang: Lang,
    pub title: String,
    pub nav: &'static str,
    pub message: Option<&'static str>,
    pub id: i64,
    pub program_url: String,
    pub program_name: String,
    pub subtitle: String,
    pub items: Vec<ItemRow>,
    pub can_add: bool,
    /// A workout in progress, which starting another one would lead to.
    pub open_workout: Option<i64>,
}

/// "3 × 8–12 reps".
fn plan_label(item: &RoutineExercise, lang: Lang) -> String {
    match item.target() {
        Some(t) => format!("{} × {}", item.sets, t.label(lang)),
        None => lang.f1("{0} sets", item.sets),
    }
}

/// "60 kg · Rest 1:30 min".
fn detail_label(
    item: &RoutineExercise,
    ex: Option<&ExerciseRef>,
    units: UnitSystem,
    lang: Lang,
) -> String {
    let mut parts = Vec::new();
    if let Some(kg) = item.weight_kg {
        let load = format!("{} {}", fmt_load_in(units, kg, lang), units.weight_unit());
        let bodyweight = ex.and_then(ExerciseRef::equipment) == Some(Equipment::Bodyweight);
        parts.push(if bodyweight { format!("+{load}") } else { load });
    }
    if let Some(r) = item.rest_s {
        parts.push(lang.f1("Rest {0}", gym::fmt_seconds(r)));
    }
    parts.join(" · ")
}

pub(super) fn fmt_load_in(units: UnitSystem, kg: f64, lang: Lang) -> String {
    gym::fmt_load(units.weight_from_kg(kg), lang)
}

pub async fn routine_detail(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<MsgQuery>,
) -> AppResult<Response> {
    let lang = current_lang();
    let (routine, program) = load_routine(&state, &id).await?;
    let units = profile::load(&state.db).await?.units();
    let list = gym::items(&state.db, routine.id).await?;
    let customs = gym::custom_exercises(&state.db).await?;
    let n = list.len();
    let items = list
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let ex = match (&item.exercise_key, item.custom_exercise_id) {
                (Some(k), _) => exercises::find(k).map(ExerciseRef::Builtin),
                (None, Some(cid)) => customs
                    .iter()
                    .find(|c| c.id == cid)
                    .cloned()
                    .map(ExerciseRef::Custom),
                _ => None,
            };
            ItemRow {
                id: item.id,
                url: format!("/gym/items/{}", item.id),
                name: ex
                    .as_ref()
                    .map_or_else(|| lang.t("Unknown exercise").into(), |e| e.name(lang)),
                plan: plan_label(item, lang),
                detail: detail_label(item, ex.as_ref(), units, lang),
                note: item.note.clone().unwrap_or_default(),
                first: i == 0,
                last: i + 1 == n,
            }
        })
        .collect();
    let subtitle = match program.schedule() {
        Schedule::Weekly => days_label(routine.days(), lang),
        Schedule::Rotation => {
            let pos = gym::routines(&state.db, program.id)
                .await?
                .iter()
                .position(|r| r.routine.id == routine.id)
                .unwrap_or(0);
            lang.f1("Session {0} of the rotation", pos + 1)
        }
    };
    let page = RoutinePage {
        lang,
        title: routine.name.clone(),
        nav: NAV,
        message: flash_message(q.msg.as_deref(), lang),
        id: routine.id,
        program_url: format!("/gym/programs/{}", program.id),
        program_name: program.name,
        subtitle,
        can_add: (n as i64) < gym::MAX_ITEMS,
        items,
        open_workout: workouts::open(&state.db).await?.map(|w| w.id),
    };
    Ok(render(&page)?.into_response())
}

#[derive(Template)]
#[template(path = "gym_routine_edit.html")]
pub struct RoutineEditPage {
    pub lang: Lang,
    pub title: String,
    pub nav: &'static str,
    pub id: i64,
    pub weekly: bool,
    pub name: String,
    pub days: Vec<DayOption>,
    pub errors: FieldErrors,
}

pub async fn edit_routine_form(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<Response> {
    let lang = current_lang();
    let (r, p) = load_routine(&state, &id).await?;
    let page = RoutineEditPage {
        lang,
        title: lang.t("Edit routine").into(),
        nav: NAV,
        id: r.id,
        weekly: p.schedule() == Schedule::Weekly,
        days: DayFields::of(r.days()).options(lang),
        name: r.name,
        errors: FieldErrors::default(),
    };
    Ok(render(&page)?.into_response())
}

pub async fn update_routine(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Form(form): Form<RoutineForm>,
) -> AppResult<Response> {
    let lang = current_lang();
    let (r, p) = load_routine(&state, &id).await?;
    let weekly = p.schedule() == Schedule::Weekly;
    let mut errors = FieldErrors::default();
    if let Some(name) = name_field(&form.name, &mut errors, lang) {
        // A rotation does not use weekdays; the ones set earlier are kept
        // for when the program goes back to a weekly schedule.
        let days = if weekly { form.days.days() } else { r.days() };
        gym::update_routine(&state.db, r.id, &name, days).await?;
        return Ok(see_other(&format!(
            "/gym/routines/{}?msg=routine_saved",
            r.id
        )));
    }
    let page = RoutineEditPage {
        lang,
        title: lang.t("Edit routine").into(),
        nav: NAV,
        id: r.id,
        weekly,
        name: form.name.clone(),
        days: form.days.options(lang),
        errors,
    };
    render_status(StatusCode::UNPROCESSABLE_ENTITY, &page)
}

pub async fn delete_routine(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<Response> {
    let (r, p) = load_routine(&state, &id).await?;
    gym::delete_routine(&state.db, r.id).await?;
    Ok(see_other(&format!(
        "/gym/programs/{}?msg=routine_deleted",
        p.id
    )))
}

#[derive(Deserialize)]
pub struct MoveForm {
    #[serde(default)]
    dir: String,
}

pub async fn move_routine(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Form(form): Form<MoveForm>,
) -> AppResult<Response> {
    let (r, p) = load_routine(&state, &id).await?;
    let dir = Move::parse(&form.dir)
        .ok_or_else(|| AppError::BadRequest(current_lang().t("Invalid request").into()))?;
    gym::move_routine(&state.db, &r, dir).await?;
    Ok(see_other(&format!(
        "/gym/programs/{}#routine-{}",
        p.id, r.id
    )))
}

// ---------- Exercise list ----------

#[derive(Clone, Debug, Default, Deserialize)]
pub struct ExerciseQuery {
    #[serde(default)]
    q: String,
    #[serde(default)]
    muscle: String,
    /// The routine an exercise is being picked for.
    #[serde(default)]
    routine: String,
    /// The workout in progress an exercise is being picked for.
    #[serde(default)]
    workout: String,
    #[serde(default)]
    msg: Option<String>,
}

/// What an exercise is being picked for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum PickFor {
    Browse,
    Routine(i64),
    Workout(i64),
}

impl PickFor {
    fn param(self) -> Option<String> {
        match self {
            PickFor::Browse => None,
            PickFor::Routine(r) => Some(format!("routine={r}")),
            PickFor::Workout(w) => Some(format!("workout={w}")),
        }
    }

    /// From the `routine` or `workout` form field; only existing routines
    /// and workouts in progress count.
    async fn load(state: &AppState, routine: &str, workout: &str) -> AppResult<PickFor> {
        if let Ok(id) = parse_id(workout)
            && let Some(w) = workouts::get(&state.db, id).await?
            && w.is_open()
        {
            return Ok(PickFor::Workout(w.id));
        }
        if let Ok(id) = parse_id(routine)
            && let Some(r) = gym::routine(&state.db, id).await?
        {
            return Ok(PickFor::Routine(r.id));
        }
        Ok(PickFor::Browse)
    }
}

pub struct MuscleChip {
    pub url: String,
    pub label: &'static str,
    pub current: bool,
}

pub struct ExerciseRow {
    /// Where choosing the row leads; empty when it is not a link.
    pub url: String,
    pub name: String,
    pub sub: String,
    pub custom: bool,
    /// Normalized names, for filtering as the user types.
    pub search: String,
    pub muscle: &'static str,
}

#[derive(Template)]
#[template(path = "gym_exercises.html")]
pub struct ExercisesPage {
    pub lang: Lang,
    pub title: String,
    pub nav: &'static str,
    pub message: Option<&'static str>,
    pub back_url: String,
    pub back_label: &'static str,
    pub routine: Option<i64>,
    pub workout: Option<i64>,
    /// Choosing a row adds the exercise to a workout, which is a form post.
    pub post: bool,
    pub q: String,
    pub muscle: String,
    pub chips: Vec<MuscleChip>,
    pub rows: Vec<ExerciseRow>,
    pub create_url: String,
}

pub(super) fn exercises_url(pick: PickFor, q: &str, muscle: Option<Muscle>) -> String {
    let mut parts = Vec::new();
    if let Some(p) = pick.param() {
        parts.push(p);
    }
    if !q.is_empty() {
        parts.push(format!("q={}", super::url_encode(q)));
    }
    if let Some(m) = muscle {
        parts.push(format!("muscle={}", m.as_str()));
    }
    if parts.is_empty() {
        "/gym/exercises".into()
    } else {
        format!("/gym/exercises?{}", parts.join("&"))
    }
}

pub(super) fn exercise_sub(
    muscle: Option<Muscle>,
    equipment: Option<Equipment>,
    lang: Lang,
) -> String {
    [
        muscle.map(|m| m.label(lang)),
        equipment.map(|e| e.label(lang)),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" · ")
}

/// Whether every word of the query starts a word of `text` (both normalized).
fn matches_query(text: &str, words: &[&str]) -> bool {
    words.iter().all(|w| {
        text.split(' ')
            .any(|t| t.starts_with(w) || (w.len() > 3 && t.contains(w)))
    })
}

pub async fn exercise_list(
    State(state): State<AppState>,
    Query(q): Query<ExerciseQuery>,
) -> AppResult<Response> {
    let lang = current_lang();
    let pick = match (q.routine.as_str(), q.workout.as_str()) {
        ("", "") => PickFor::Browse,
        (r, "") => PickFor::Routine(load_routine(&state, r).await?.0.id),
        (_, w) => {
            let w = workouts::get(&state.db, parse_id(w)?)
                .await?
                .ok_or(AppError::NotFound)?;
            if !w.is_open() {
                return Ok(see_other(&format!("/gym/workouts/{}", w.id)));
            }
            PickFor::Workout(w.id)
        }
    };
    let query: String = q.q.trim().chars().take(100).collect();
    let muscle = Muscle::parse(&q.muscle);
    let needle = normalize(&query);
    let words: Vec<&str> = needle.split(' ').filter(|w| !w.is_empty()).collect();
    let keep = |text: &str, m: Option<Muscle>| {
        muscle.is_none_or(|want| m == Some(want)) && matches_query(text, &words)
    };

    let mut rows = Vec::new();
    for c in gym::custom_exercises(&state.db).await? {
        let m = Muscle::parse(&c.muscle);
        let search = normalize(&c.name);
        if !keep(&search, m) {
            continue;
        }
        rows.push(ExerciseRow {
            url: match pick {
                PickFor::Routine(r) => format!("/gym/routines/{r}/items/new?custom={}", c.id),
                PickFor::Workout(w) => format!("/gym/workouts/{w}/exercises?custom={}", c.id),
                PickFor::Browse => format!("/gym/exercises/{}/edit", c.id),
            },
            name: c.name.clone(),
            sub: exercise_sub(m, Equipment::parse(&c.equipment), lang),
            custom: true,
            search,
            muscle: m.map_or("", Muscle::as_str),
        });
    }
    for e in exercises::sorted(lang) {
        let search = e.search_text();
        if !keep(&search, Some(e.muscle)) {
            continue;
        }
        rows.push(ExerciseRow {
            url: match pick {
                PickFor::Routine(r) => format!("/gym/routines/{r}/items/new?exercise={}", e.key),
                PickFor::Workout(w) => format!("/gym/workouts/{w}/exercises?exercise={}", e.key),
                PickFor::Browse => String::new(),
            },
            name: e.name(lang).into(),
            sub: exercise_sub(Some(e.muscle), Some(e.equipment), lang),
            custom: false,
            search,
            muscle: e.muscle.as_str(),
        });
    }
    let mut chips = vec![MuscleChip {
        url: exercises_url(pick, &query, None),
        label: lang.t("All"),
        current: muscle.is_none(),
    }];
    chips.extend(Muscle::ALL.into_iter().map(|m| MuscleChip {
        url: exercises_url(pick, &query, Some(m)),
        label: m.label(lang),
        current: muscle == Some(m),
    }));
    let page = ExercisesPage {
        lang,
        title: if pick == PickFor::Browse {
            lang.t("Exercises").into()
        } else {
            lang.t("Add exercise").into()
        },
        nav: NAV,
        message: flash_message(q.msg.as_deref(), lang),
        back_url: match pick {
            PickFor::Browse => "/gym".into(),
            PickFor::Routine(r) => format!("/gym/routines/{r}"),
            PickFor::Workout(w) => format!("/gym/workouts/{w}"),
        },
        back_label: match pick {
            PickFor::Browse => lang.t("Back to gym"),
            PickFor::Routine(_) => lang.t("Back to routine"),
            PickFor::Workout(_) => lang.t("Back to workout"),
        },
        routine: match pick {
            PickFor::Routine(r) => Some(r),
            _ => None,
        },
        workout: match pick {
            PickFor::Workout(w) => Some(w),
            _ => None,
        },
        post: matches!(pick, PickFor::Workout(_)),
        q: query,
        muscle: muscle.map_or("", Muscle::as_str).into(),
        chips,
        rows,
        create_url: match pick.param() {
            Some(p) => format!("/gym/exercises/new?{p}"),
            None => "/gym/exercises/new".into(),
        },
    };
    Ok(render(&page)?.into_response())
}

// ---------- Routine exercises ----------

#[derive(Clone, Debug, Default, Deserialize)]
pub struct ItemForm {
    #[serde(default)]
    sets: String,
    #[serde(default)]
    reps: String,
    #[serde(default)]
    duration: String,
    #[serde(default)]
    weight: String,
    #[serde(default)]
    rest: String,
    #[serde(default)]
    note: String,
}

impl ItemForm {
    fn from_item(item: &RoutineExercise, units: UnitSystem, lang: Lang) -> Self {
        ItemForm {
            sets: item.sets.to_string(),
            reps: match (item.reps_min, item.reps_max) {
                (Some(min), Some(max)) => format!("{min}-{max}"),
                (Some(min), None) => min.to_string(),
                _ => String::new(),
            },
            duration: item.duration_s.map(gym::seconds_input).unwrap_or_default(),
            weight: item
                .weight_kg
                .map(|kg| fmt_load_in(units, kg, lang))
                .unwrap_or_default(),
            rest: item.rest_s.map(gym::seconds_input).unwrap_or_default(),
            note: item.note.clone().unwrap_or_default(),
        }
    }

    fn blank(timed: bool) -> Self {
        ItemForm {
            sets: "3".into(),
            reps: if timed { String::new() } else { "10".into() },
            duration: if timed { "30".into() } else { String::new() },
            rest: "90".into(),
            ..Default::default()
        }
    }
}

fn validate_item(
    form: &ItemForm,
    timed: bool,
    units: UnitSystem,
    lang: Lang,
) -> Result<ItemInput, FieldErrors> {
    let mut errors = FieldErrors::default();
    let sets = match form.sets.trim().parse::<i64>() {
        Ok(n) if (1..=gym::MAX_SETS).contains(&n) => Some(n),
        _ if form.sets.trim().is_empty() => {
            errors.add("sets", lang.t("Enter the number of sets."));
            None
        }
        _ => {
            errors.add(
                "sets",
                lang.f1("Enter between 1 and {0} sets.", gym::MAX_SETS),
            );
            None
        }
    };
    let target = if timed {
        match gym::parse_seconds(&form.duration, gym::MAX_DURATION_S) {
            Ok(Some(s)) if s > 0 => Some(Target::Duration(s)),
            Ok(_) => {
                errors.add(
                    "duration",
                    lang.t("Enter the time of each set, e.g. 45 or 1:30."),
                );
                None
            }
            Err(_) => {
                errors.add(
                    "duration",
                    lang.t("Enter a time up to 60 minutes, e.g. 45 or 1:30."),
                );
                None
            }
        }
    } else {
        match gym::parse_reps(&form.reps) {
            Ok(Some((min, max))) => Some(Target::Reps { min, max }),
            Ok(None) => {
                errors.add("reps", lang.t("Enter the reps, e.g. 8 or 8-12."));
                None
            }
            Err(ParseError::OutOfRange) => {
                errors.add(
                    "reps",
                    lang.f1(
                        "Enter reps between 1 and {0}, e.g. 8 or 8-12.",
                        gym::MAX_REPS,
                    ),
                );
                None
            }
            Err(ParseError::Invalid) => {
                errors.add(
                    "reps",
                    lang.t("Enter a number of reps or a range from low to high, e.g. 8 or 8-12."),
                );
                None
            }
        }
    };
    let weight_kg = match parse_decimal(&form.weight) {
        Ok(None) => None,
        Ok(Some(v)) => {
            let kg = units.weight_to_kg(v);
            if (0.0..=gym::MAX_LOAD_KG).contains(&kg) {
                Some(kg)
            } else {
                errors.add(
                    "weight",
                    lang.f2(
                        "Enter a weight between 0 and {0} {1}.",
                        gym::fmt_load(units.weight_from_kg(gym::MAX_LOAD_KG).floor(), lang),
                        units.weight_unit(),
                    ),
                );
                None
            }
        }
        Err(e) => {
            errors.add("weight", e.for_field(lang, lang.t("Weight")));
            None
        }
    };
    let rest_s = match gym::parse_seconds(&form.rest, gym::MAX_REST_S) {
        Ok(v) => v,
        Err(_) => {
            errors.add(
                "rest",
                lang.t("Enter a rest time up to 15 minutes, e.g. 90 or 1:30."),
            );
            None
        }
    };
    let note = clean_text(&form.note, gym::MAX_NOTE_CHARS).unwrap_or_else(|e| {
        errors.add("note", e.for_field(lang, lang.t("Note")));
        None
    });
    match (sets, target) {
        (Some(sets), Some(target)) if errors.is_empty() => Ok(ItemInput {
            sets,
            target,
            weight_kg,
            rest_s,
            note,
        }),
        _ => Err(errors),
    }
}

#[derive(Template)]
#[template(path = "gym_item_form.html")]
pub struct ItemFormPage {
    pub lang: Lang,
    pub title: String,
    pub nav: &'static str,
    pub exercise: String,
    pub exercise_sub: String,
    /// Where the form posts.
    pub action: String,
    pub back_url: String,
    /// The item being edited; `None` when adding.
    pub item_id: Option<i64>,
    pub timed: bool,
    pub bodyweight: bool,
    pub unit: &'static str,
    pub form: ItemForm,
    pub errors: FieldErrors,
}

fn item_form_page(
    ex: &ExerciseRef,
    action: String,
    back_url: String,
    item_id: Option<i64>,
    units: UnitSystem,
    form: ItemForm,
    errors: FieldErrors,
) -> ItemFormPage {
    let lang = current_lang();
    ItemFormPage {
        lang,
        title: ex.name(lang),
        nav: NAV,
        exercise: ex.name(lang),
        exercise_sub: exercise_sub(ex.muscle(), ex.equipment(), lang),
        action,
        back_url,
        item_id,
        timed: ex.timed(),
        bodyweight: ex.equipment() == Some(Equipment::Bodyweight),
        unit: units.weight_unit(),
        form,
        errors,
    }
}

#[derive(Default, Deserialize)]
pub struct PickQuery {
    #[serde(default)]
    exercise: String,
    #[serde(default)]
    custom: String,
}

impl PickQuery {
    fn custom(id: i64) -> Self {
        PickQuery {
            custom: id.to_string(),
            ..Default::default()
        }
    }
}

pub(super) async fn picked(state: &AppState, q: &PickQuery) -> AppResult<ExerciseRef> {
    if !q.exercise.is_empty() {
        return exercises::find(&q.exercise)
            .map(ExerciseRef::Builtin)
            .ok_or(AppError::NotFound);
    }
    let c = gym::custom_exercise(&state.db, parse_id(&q.custom)?)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(ExerciseRef::Custom(c))
}

pub async fn new_item_form(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<PickQuery>,
) -> AppResult<Response> {
    let (r, _) = load_routine(&state, &id).await?;
    let ex = picked(&state, &q).await?;
    let units = profile::load(&state.db).await?.units();
    let page = item_form_page(
        &ex,
        format!("/gym/routines/{}/items?{}", r.id, ex.query()),
        exercises_url(PickFor::Routine(r.id), "", None),
        None,
        units,
        ItemForm::blank(ex.timed()),
        FieldErrors::default(),
    );
    Ok(render(&page)?.into_response())
}

pub async fn create_item(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<PickQuery>,
    Form(form): Form<ItemForm>,
) -> AppResult<Response> {
    let lang = current_lang();
    let (r, _) = load_routine(&state, &id).await?;
    let ex = picked(&state, &q).await?;
    if gym::item_count(&state.db, r.id).await? >= gym::MAX_ITEMS {
        return Err(AppError::BadRequest(
            lang.f1("A routine can hold up to {0} exercises.", gym::MAX_ITEMS),
        ));
    }
    let units = profile::load(&state.db).await?.units();
    match validate_item(&form, ex.timed(), units, lang) {
        Ok(v) => {
            gym::add_item(&state.db, r.id, &ex, &v).await?;
            Ok(see_other(&format!(
                "/gym/routines/{}?msg=exercise_added",
                r.id
            )))
        }
        Err(errors) => render_status(
            StatusCode::UNPROCESSABLE_ENTITY,
            &item_form_page(
                &ex,
                format!("/gym/routines/{}/items?{}", r.id, ex.query()),
                exercises_url(PickFor::Routine(r.id), "", None),
                None,
                units,
                form,
                errors,
            ),
        ),
    }
}

async fn load_item(state: &AppState, id: &str) -> AppResult<(RoutineExercise, ExerciseRef)> {
    let item = gym::item(&state.db, parse_id(id)?)
        .await?
        .ok_or(AppError::NotFound)?;
    // A built-in exercise is never removed from the list, so this only
    // fails for a database edited by hand.
    let ex = gym::exercise_of(&state.db, &item)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok((item, ex))
}

pub async fn edit_item_form(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<Response> {
    let lang = current_lang();
    let (item, ex) = load_item(&state, &id).await?;
    let units = profile::load(&state.db).await?.units();
    let page = item_form_page(
        &ex,
        format!("/gym/items/{}", item.id),
        format!("/gym/routines/{}", item.routine_id),
        Some(item.id),
        units,
        ItemForm::from_item(&item, units, lang),
        FieldErrors::default(),
    );
    Ok(render(&page)?.into_response())
}

pub async fn update_item(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Form(form): Form<ItemForm>,
) -> AppResult<Response> {
    let (item, ex) = load_item(&state, &id).await?;
    let units = profile::load(&state.db).await?.units();
    // The stored plan decides how sets are counted, so a custom exercise
    // changed to timed later keeps its reps here.
    let timed = item.duration_s.is_some();
    match validate_item(&form, timed, units, current_lang()) {
        Ok(v) => {
            gym::update_item(&state.db, item.id, &v).await?;
            Ok(see_other(&format!(
                "/gym/routines/{}?msg=exercise_saved",
                item.routine_id
            )))
        }
        Err(errors) => {
            let mut page = item_form_page(
                &ex,
                format!("/gym/items/{}", item.id),
                format!("/gym/routines/{}", item.routine_id),
                Some(item.id),
                units,
                form,
                errors,
            );
            page.timed = timed;
            render_status(StatusCode::UNPROCESSABLE_ENTITY, &page)
        }
    }
}

pub async fn delete_item(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<Response> {
    let item = gym::item(&state.db, parse_id(&id)?)
        .await?
        .ok_or(AppError::NotFound)?;
    gym::delete_item(&state.db, item.id).await?;
    Ok(see_other(&format!(
        "/gym/routines/{}?msg=exercise_removed",
        item.routine_id
    )))
}

pub async fn move_item(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Form(form): Form<MoveForm>,
) -> AppResult<Response> {
    let item = gym::item(&state.db, parse_id(&id)?)
        .await?
        .ok_or(AppError::NotFound)?;
    let dir = Move::parse(&form.dir)
        .ok_or_else(|| AppError::BadRequest(current_lang().t("Invalid request").into()))?;
    gym::move_item(&state.db, &item, dir).await?;
    Ok(see_other(&format!(
        "/gym/routines/{}#item-{}",
        item.routine_id, item.id
    )))
}

// ---------- Custom exercises ----------

#[derive(Clone, Debug, Default, Deserialize)]
pub struct ExerciseForm {
    #[serde(default)]
    name: String,
    #[serde(default)]
    muscle: String,
    #[serde(default)]
    equipment: String,
    #[serde(default)]
    timed: String,
    /// The routine to add the new exercise to once created.
    #[serde(default)]
    routine: String,
    /// The workout in progress to add the new exercise to once created.
    #[serde(default)]
    workout: String,
}

#[derive(Template)]
#[template(path = "gym_exercise_form.html")]
pub struct ExerciseFormPage {
    pub lang: Lang,
    pub title: String,
    pub nav: &'static str,
    pub id: Option<i64>,
    pub back_url: String,
    /// The hidden `routine` or `workout` field carrying where the exercise goes.
    pub pick: Option<(&'static str, i64)>,
    pub name: String,
    pub muscles: Vec<MenuOption>,
    pub equipment: Vec<MenuOption>,
    pub timed: bool,
    /// Routine exercises using it; its timing can then no longer change.
    pub usage: i64,
    pub errors: FieldErrors,
}

fn exercise_form_page(
    id: Option<i64>,
    pick: PickFor,
    form: &ExerciseForm,
    usage: i64,
    errors: FieldErrors,
) -> ExerciseFormPage {
    let lang = current_lang();
    let choose = |selected: bool| MenuOption {
        value: "",
        label: lang.t("Choose"),
        icon: "",
        selected,
    };
    let mut muscles = Vec::new();
    if Muscle::parse(&form.muscle).is_none() {
        muscles.push(choose(true));
    }
    muscles.extend(Muscle::ALL.into_iter().map(|m| MenuOption {
        value: m.as_str(),
        label: m.label(lang),
        icon: "",
        selected: form.muscle == m.as_str(),
    }));
    let mut equipment = Vec::new();
    if Equipment::parse(&form.equipment).is_none() {
        equipment.push(choose(true));
    }
    equipment.extend(Equipment::ALL.into_iter().map(|e| MenuOption {
        value: e.as_str(),
        label: e.label(lang),
        icon: "",
        selected: form.equipment == e.as_str(),
    }));
    ExerciseFormPage {
        lang,
        title: if id.is_some() {
            lang.t("Edit exercise").into()
        } else {
            lang.t("New exercise").into()
        },
        nav: NAV,
        id,
        back_url: exercises_url(pick, "", None),
        pick: match pick {
            PickFor::Browse => None,
            PickFor::Routine(r) => Some(("routine", r)),
            PickFor::Workout(w) => Some(("workout", w)),
        },
        name: form.name.clone(),
        muscles,
        equipment,
        timed: !form.timed.is_empty(),
        usage,
        errors,
    }
}

#[derive(Deserialize)]
pub struct RoutineQuery {
    #[serde(default)]
    routine: String,
    #[serde(default)]
    workout: String,
}

pub async fn new_exercise_form(
    State(state): State<AppState>,
    Query(q): Query<RoutineQuery>,
) -> AppResult<Response> {
    let pick = PickFor::load(&state, &q.routine, &q.workout).await?;
    let page = exercise_form_page(
        None,
        pick,
        &ExerciseForm::default(),
        0,
        FieldErrors::default(),
    );
    Ok(render(&page)?.into_response())
}

fn validate_exercise(form: &ExerciseForm, lang: Lang) -> Result<CustomInput, FieldErrors> {
    let mut errors = FieldErrors::default();
    let name = name_field(&form.name, &mut errors, lang);
    let muscle = Muscle::parse(&form.muscle);
    if muscle.is_none() {
        errors.add("muscle", lang.t("Choose a muscle group."));
    }
    let equipment = Equipment::parse(&form.equipment);
    if equipment.is_none() {
        errors.add("equipment", lang.t("Choose the equipment."));
    }
    match (name, muscle, equipment) {
        (Some(name), Some(muscle), Some(equipment)) => Ok(CustomInput {
            name,
            muscle,
            equipment,
            timed: !form.timed.is_empty(),
        }),
        _ => Err(errors),
    }
}

pub async fn create_exercise(
    State(state): State<AppState>,
    Form(form): Form<ExerciseForm>,
) -> AppResult<Response> {
    let lang = current_lang();
    if gym::custom_exercise_count(&state.db).await? >= gym::MAX_CUSTOM_EXERCISES {
        return Err(AppError::BadRequest(lang.f1(
            "You can add up to {0} exercises of your own.",
            gym::MAX_CUSTOM_EXERCISES,
        )));
    }
    let pick = PickFor::load(&state, &form.routine, &form.workout).await?;
    match validate_exercise(&form, lang) {
        Ok(c) => {
            let id = gym::create_custom_exercise(&state.db, &c).await?;
            Ok(see_other(&match pick {
                PickFor::Routine(r) => format!("/gym/routines/{r}/items/new?custom={id}"),
                PickFor::Workout(w) => {
                    super::gym_workouts::add_picked(&state, w, PickQuery::custom(id), lang).await?
                }
                PickFor::Browse => "/gym/exercises?msg=exercise_created".into(),
            }))
        }
        Err(errors) => render_status(
            StatusCode::UNPROCESSABLE_ENTITY,
            &exercise_form_page(None, pick, &form, 0, errors),
        ),
    }
}

async fn load_custom(state: &AppState, id: &str) -> AppResult<gym::CustomExercise> {
    gym::custom_exercise(&state.db, parse_id(id)?)
        .await?
        .ok_or(AppError::NotFound)
}

pub async fn edit_exercise_form(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<Response> {
    let c = load_custom(&state, &id).await?;
    let usage = gym::custom_exercise_usage(&state.db, c.id).await?;
    let form = ExerciseForm {
        name: c.name,
        muscle: c.muscle,
        equipment: c.equipment,
        timed: if c.timed { "on".into() } else { String::new() },
        ..Default::default()
    };
    Ok(render(&exercise_form_page(
        Some(c.id),
        PickFor::Browse,
        &form,
        usage,
        FieldErrors::default(),
    ))?
    .into_response())
}

pub async fn update_exercise(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Form(form): Form<ExerciseForm>,
) -> AppResult<Response> {
    let c = load_custom(&state, &id).await?;
    match validate_exercise(&form, current_lang()) {
        Ok(input) => {
            gym::update_custom_exercise(&state.db, c.id, &input).await?;
            Ok(see_other("/gym/exercises?msg=exercise_updated"))
        }
        Err(errors) => {
            let usage = gym::custom_exercise_usage(&state.db, c.id).await?;
            render_status(
                StatusCode::UNPROCESSABLE_ENTITY,
                &exercise_form_page(Some(c.id), PickFor::Browse, &form, usage, errors),
            )
        }
    }
}

pub async fn delete_exercise(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<Response> {
    let lang = current_lang();
    let id = parse_id(&id)?;
    match gym::delete_custom_exercise(&state.db, id).await? {
        DeleteResult::Deleted => Ok(see_other("/gym/exercises?msg=exercise_deleted")),
        DeleteResult::NotFound => Err(AppError::NotFound),
        DeleteResult::InUse => Err(AppError::BadRequest(
            lang.t("This exercise is used in a routine. Remove it from your routines first.")
                .into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form(sets: &str, reps: &str, weight: &str, rest: &str) -> ItemForm {
        ItemForm {
            sets: sets.into(),
            reps: reps.into(),
            weight: weight.into(),
            rest: rest.into(),
            ..Default::default()
        }
    }

    #[test]
    fn items_are_validated_and_loads_stored_in_kg() {
        let v = validate_item(
            &form("4", "8-12", "100", "1:30"),
            false,
            UnitSystem::Imperial,
            Lang::En,
        )
        .unwrap();
        assert_eq!(v.sets, 4);
        assert_eq!(
            v.target,
            Target::Reps {
                min: 8,
                max: Some(12)
            }
        );
        assert!((v.weight_kg.unwrap() - 45.359237).abs() < 1e-9);
        assert_eq!(v.rest_s, Some(90));

        let e = validate_item(
            &form("0", "", "-5", "20:00"),
            false,
            UnitSystem::Metric,
            Lang::En,
        )
        .unwrap_err();
        for f in ["sets", "reps", "weight", "rest"] {
            assert!(e.has(f), "{f}");
        }
        assert!(
            validate_item(
                &form("3", "10", "1001", ""),
                false,
                UnitSystem::Metric,
                Lang::En
            )
            .is_err()
        );
        let ok = validate_item(
            &form("3", "10", "", ""),
            false,
            UnitSystem::Metric,
            Lang::En,
        )
        .unwrap();
        assert_eq!((ok.weight_kg, ok.rest_s), (None, None));
    }

    #[test]
    fn timed_items_ask_for_a_time_instead_of_reps() {
        let mut f = form("3", "", "", "60");
        assert!(
            validate_item(&f, true, UnitSystem::Metric, Lang::En)
                .unwrap_err()
                .has("duration")
        );
        f.duration = "0:45".into();
        let v = validate_item(&f, true, UnitSystem::Metric, Lang::En).unwrap();
        assert_eq!(v.target, Target::Duration(45));
    }

    #[test]
    fn exercise_search_matches_word_starts_in_any_language() {
        let bench = exercises::find("barbell_bench_press")
            .unwrap()
            .search_text();
        assert!(matches_query(&bench, &["bench"]));
        assert!(matches_query(&bench, &["dev", "couche"]));
        assert!(matches_query(&bench, &["banca"]));
        assert!(!matches_query(&bench, &["squat"]));
        assert!(matches_query(&bench, &[]));
    }

    #[test]
    fn day_labels_are_short_or_say_none() {
        assert_eq!(days_label(Weekdays::NONE, Lang::En), "No day chosen");
        assert_eq!(
            days_label(Weekdays::NONE.with(0).with(3), Lang::En),
            "Mon, Thu"
        );
        assert_eq!(days_label(Weekdays::ALL, Lang::En), "Every day");
    }
}
