//! Gym programs: routines of exercises with their planned sets, reps, load
//! and rest, scheduled on weekdays or done in rotation. What was actually
//! done is logged as workouts (see [`workouts`]). Loads are stored in
//! kilograms and converted for display.

pub mod exercises;
pub mod starter;
pub mod workouts;

use serde::{Deserialize, Serialize};
use sqlx::{AssertSqlSafe, SqliteConnection, SqlitePool};

use crate::activity::Weekdays;
use crate::error::AppResult;
use crate::i18n::Lang;
use exercises::{Equipment, Exercise, Muscle};

pub const MAX_NAME_CHARS: usize = 80;
pub const MAX_NOTE_CHARS: usize = 200;
pub const MAX_SETS: i64 = 20;
pub const MAX_REPS: i64 = 100;
pub const MAX_DURATION_S: i64 = 3600;
pub const MAX_REST_S: i64 = 900;
pub const MAX_LOAD_KG: f64 = 1000.0;
/// Bounds that keep pages and imports small.
pub const MAX_PROGRAMS: i64 = 50;
pub const MAX_ROUTINES: i64 = 30;
pub const MAX_ITEMS: i64 = 40;
pub const MAX_CUSTOM_EXERCISES: i64 = 500;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Schedule {
    /// Each routine on chosen days of the week.
    Weekly,
    /// Routines in order, the next one whenever you train.
    Rotation,
}

impl Schedule {
    pub fn as_str(self) -> &'static str {
        match self {
            Schedule::Weekly => "weekly",
            Schedule::Rotation => "rotation",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "weekly" => Some(Schedule::Weekly),
            "rotation" => Some(Schedule::Rotation),
            _ => None,
        }
    }

    pub fn label(self, lang: Lang) -> &'static str {
        match self {
            Schedule::Weekly => lang.t("Days of the week"),
            Schedule::Rotation => lang.t("Rotation"),
        }
    }
}

#[derive(Clone, Debug, sqlx::FromRow, Serialize, Deserialize)]
pub struct Program {
    pub id: i64,
    pub name: String,
    pub schedule: String,
    pub active: bool,
}

impl Program {
    pub fn schedule(&self) -> Schedule {
        Schedule::parse(&self.schedule).unwrap_or(Schedule::Weekly)
    }
}

#[derive(Clone, Debug, sqlx::FromRow, Serialize, Deserialize)]
pub struct Routine {
    pub id: i64,
    pub program_id: i64,
    pub name: String,
    pub position: i64,
    /// [`Weekdays`] bits, used by weekly programs.
    pub weekdays: i64,
}

impl Routine {
    pub fn days(&self) -> Weekdays {
        Weekdays::from_bits(self.weekdays).unwrap_or_default()
    }
}

#[derive(Clone, Debug, sqlx::FromRow, Serialize, Deserialize)]
pub struct CustomExercise {
    pub id: i64,
    pub name: String,
    pub muscle: String,
    pub equipment: String,
    pub timed: bool,
}

/// One exercise of a routine with its plan.
#[derive(Clone, Debug, sqlx::FromRow, Serialize, Deserialize)]
pub struct RoutineExercise {
    pub id: i64,
    pub routine_id: i64,
    pub position: i64,
    pub exercise_key: Option<String>,
    pub custom_exercise_id: Option<i64>,
    pub sets: i64,
    pub reps_min: Option<i64>,
    pub reps_max: Option<i64>,
    pub duration_s: Option<i64>,
    pub weight_kg: Option<f64>,
    pub rest_s: Option<i64>,
    pub note: Option<String>,
}

impl RoutineExercise {
    pub fn target(&self) -> Option<Target> {
        match (self.reps_min, self.reps_max, self.duration_s) {
            (Some(min), max, None) => Some(Target::Reps { min, max }),
            (None, None, Some(s)) => Some(Target::Duration(s)),
            _ => None,
        }
    }
}

/// What one set asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// A number of reps, or a range such as 8-12.
    Reps { min: i64, max: Option<i64> },
    /// Seconds to hold or keep going.
    Duration(i64),
}

impl Target {
    /// "8 reps", "8-12 reps", "45 s", "1:30 min".
    pub fn label(self, lang: Lang) -> String {
        match self {
            Target::Reps { min: 1, max: None } => lang.t("1 rep").into(),
            Target::Reps { min, max: None } => lang.f1("{0} reps", min),
            Target::Reps {
                min,
                max: Some(max),
            } => lang.f1("{0} reps", format!("{min}–{max}")),
            Target::Duration(s) => fmt_seconds(s),
        }
    }
}

/// A built-in exercise or one of the user's.
#[derive(Clone, Debug)]
pub enum ExerciseRef {
    Builtin(&'static Exercise),
    Custom(CustomExercise),
}

impl ExerciseRef {
    pub fn name(&self, lang: Lang) -> String {
        match self {
            ExerciseRef::Builtin(e) => e.name(lang).into(),
            ExerciseRef::Custom(c) => c.name.clone(),
        }
    }

    pub fn muscle(&self) -> Option<Muscle> {
        match self {
            ExerciseRef::Builtin(e) => Some(e.muscle),
            ExerciseRef::Custom(c) => Muscle::parse(&c.muscle),
        }
    }

    pub fn equipment(&self) -> Option<Equipment> {
        match self {
            ExerciseRef::Builtin(e) => Some(e.equipment),
            ExerciseRef::Custom(c) => Equipment::parse(&c.equipment),
        }
    }

    pub fn timed(&self) -> bool {
        match self {
            ExerciseRef::Builtin(e) => e.timed,
            ExerciseRef::Custom(c) => c.timed,
        }
    }

    /// Query string selecting this exercise, e.g. `exercise=plank` or `custom=4`.
    pub fn query(&self) -> String {
        match self {
            ExerciseRef::Builtin(e) => format!("exercise={}", e.key),
            ExerciseRef::Custom(c) => format!("custom={}", c.id),
        }
    }
}

/// "45 s", "1:30 min", "2 min".
pub fn fmt_seconds(s: i64) -> String {
    match (s / 60, s % 60) {
        (0, sec) => format!("{sec} s"),
        (min, 0) => format!("{min} min"),
        (min, sec) => format!("{min}:{sec:02} min"),
    }
}

/// Seconds as typed back into a form: "45", "1:30".
pub fn seconds_input(s: i64) -> String {
    if s < 60 {
        s.to_string()
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// A load with up to two decimals ("60", "62.5", "1.25"), in the
/// language's number format.
pub fn fmt_load(v: f64, lang: Lang) -> String {
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    lang.num(s.to_string())
}

/// Why a typed value was refused; turned into a message by the form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseError {
    Invalid,
    OutOfRange,
}

/// Reps as typed: "8", or a range "8-12" (also with an en dash or spaces).
/// A range whose ends are equal is a plain number.
pub fn parse_reps(input: &str) -> Result<Option<(i64, Option<i64>)>, ParseError> {
    let t = input.trim();
    if t.is_empty() {
        return Ok(None);
    }
    if t.len() > 16 {
        return Err(ParseError::Invalid);
    }
    let num = |s: &str| s.trim().parse::<i64>().map_err(|_| ParseError::Invalid);
    let (min, max) = match t.split_once(['-', '–', '—']) {
        Some((a, b)) => (num(a)?, Some(num(b)?)),
        None => (num(t)?, None),
    };
    let in_range = |v: i64| (1..=MAX_REPS).contains(&v);
    if !in_range(min) || max.is_some_and(|m| !in_range(m)) {
        return Err(ParseError::OutOfRange);
    }
    match max {
        Some(m) if m < min => Err(ParseError::Invalid),
        Some(m) if m == min => Ok(Some((min, None))),
        _ => Ok(Some((min, max))),
    }
}

/// A time as typed: seconds ("45") or minutes and seconds ("1:30").
pub fn parse_seconds(input: &str, max: i64) -> Result<Option<i64>, ParseError> {
    let t = input.trim();
    if t.is_empty() {
        return Ok(None);
    }
    if t.len() > 8 {
        return Err(ParseError::Invalid);
    }
    let num = |s: &str| {
        if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
            Err(ParseError::Invalid)
        } else {
            s.parse::<i64>().map_err(|_| ParseError::Invalid)
        }
    };
    let s = match t.split_once(':') {
        Some((m, s)) => {
            let sec = num(s)?;
            if s.len() != 2 || sec >= 60 {
                return Err(ParseError::Invalid);
            }
            num(m)? * 60 + sec
        }
        None => num(t)?,
    };
    if s > max {
        return Err(ParseError::OutOfRange);
    }
    Ok(Some(s))
}

/// A routine exercise as validated by the form, before it is stored.
#[derive(Clone, Debug, PartialEq)]
pub struct ItemInput {
    pub sets: i64,
    pub target: Target,
    pub weight_kg: Option<f64>,
    pub rest_s: Option<i64>,
    pub note: Option<String>,
}

impl ItemInput {
    fn reps(&self) -> (Option<i64>, Option<i64>, Option<i64>) {
        match self.target {
            Target::Reps { min, max } => (Some(min), max, None),
            Target::Duration(s) => (None, None, Some(s)),
        }
    }
}

// ---------- Programs ----------

pub struct ProgramSummary {
    pub program: Program,
    pub routines: i64,
}

pub async fn programs(db: &SqlitePool) -> AppResult<Vec<ProgramSummary>> {
    let rows: Vec<(i64, String, String, bool, i64)> = sqlx::query_as(
        "SELECT p.id, p.name, p.schedule, p.active,
                (SELECT COUNT(*) FROM gym_routines r WHERE r.program_id = p.id)
         FROM gym_programs p ORDER BY p.active DESC, p.name COLLATE NOCASE, p.id
         LIMIT ?",
    )
    .bind(MAX_PROGRAMS)
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, name, schedule, active, routines)| ProgramSummary {
            program: Program {
                id,
                name,
                schedule,
                active,
            },
            routines,
        })
        .collect())
}

pub async fn program(db: &SqlitePool, id: i64) -> AppResult<Option<Program>> {
    Ok(sqlx::query_as::<_, Program>(
        "SELECT id, name, schedule, active FROM gym_programs WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(db)
    .await?)
}

pub async fn active_program(db: &SqlitePool) -> AppResult<Option<Program>> {
    Ok(sqlx::query_as::<_, Program>(
        "SELECT id, name, schedule, active FROM gym_programs WHERE active = 1",
    )
    .fetch_optional(db)
    .await?)
}

pub async fn program_count(db: &SqlitePool) -> AppResult<i64> {
    Ok(sqlx::query_scalar("SELECT COUNT(*) FROM gym_programs")
        .fetch_one(db)
        .await?)
}

/// Creates a program; it becomes the active one when none is.
async fn insert_program(
    conn: &mut SqliteConnection,
    name: &str,
    schedule: Schedule,
) -> AppResult<i64> {
    let id = sqlx::query_scalar(
        "INSERT INTO gym_programs (name, schedule, active)
         VALUES (?, ?, NOT EXISTS (SELECT 1 FROM gym_programs WHERE active = 1))
         RETURNING id",
    )
    .bind(name)
    .bind(schedule.as_str())
    .fetch_one(&mut *conn)
    .await?;
    Ok(id)
}

pub async fn create_program(db: &SqlitePool, name: &str, schedule: Schedule) -> AppResult<i64> {
    let mut conn = db.acquire().await?;
    insert_program(&mut conn, name, schedule).await
}

pub async fn update_program(
    db: &SqlitePool,
    id: i64,
    name: &str,
    schedule: Schedule,
) -> AppResult<bool> {
    let res = sqlx::query(
        "UPDATE gym_programs SET name = ?, schedule = ?,
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
         WHERE id = ?",
    )
    .bind(name)
    .bind(schedule.as_str())
    .bind(id)
    .execute(db)
    .await?;
    Ok(res.rows_affected() > 0)
}

pub async fn activate_program(db: &SqlitePool, id: i64) -> AppResult<bool> {
    let mut tx = db.begin().await?;
    sqlx::query("UPDATE gym_programs SET active = 0 WHERE active = 1 AND id <> ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    let res = sqlx::query("UPDATE gym_programs SET active = 1 WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    if res.rows_affected() == 0 {
        return Ok(false);
    }
    tx.commit().await?;
    Ok(true)
}

/// Deletes a program with its routines. When it was the active one, the
/// most recently created remaining program takes its place.
pub async fn delete_program(db: &SqlitePool, id: i64) -> AppResult<bool> {
    let mut tx = db.begin().await?;
    let res = sqlx::query("DELETE FROM gym_programs WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "UPDATE gym_programs SET active = 1
         WHERE id = (SELECT MAX(id) FROM gym_programs)
           AND NOT EXISTS (SELECT 1 FROM gym_programs WHERE active = 1)",
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(res.rows_affected() > 0)
}

// ---------- Routines ----------

pub struct RoutineSummary {
    pub routine: Routine,
    pub exercises: i64,
}

/// A program's routines in order, with how many exercises each holds.
pub async fn routines(db: &SqlitePool, program_id: i64) -> AppResult<Vec<RoutineSummary>> {
    let rows: Vec<(i64, i64, String, i64, i64, i64)> = sqlx::query_as(
        "SELECT r.id, r.program_id, r.name, r.position, r.weekdays,
                (SELECT COUNT(*) FROM gym_routine_exercises x WHERE x.routine_id = r.id)
         FROM gym_routines r WHERE r.program_id = ? ORDER BY r.position, r.id",
    )
    .bind(program_id)
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(
            |(id, program_id, name, position, weekdays, exercises)| RoutineSummary {
                routine: Routine {
                    id,
                    program_id,
                    name,
                    position,
                    weekdays,
                },
                exercises,
            },
        )
        .collect())
}

pub async fn routine(db: &SqlitePool, id: i64) -> AppResult<Option<Routine>> {
    Ok(sqlx::query_as::<_, Routine>(
        "SELECT id, program_id, name, position, weekdays FROM gym_routines WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(db)
    .await?)
}

pub async fn routine_count(db: &SqlitePool, program_id: i64) -> AppResult<i64> {
    Ok(
        sqlx::query_scalar("SELECT COUNT(*) FROM gym_routines WHERE program_id = ?")
            .bind(program_id)
            .fetch_one(db)
            .await?,
    )
}

async fn insert_routine(
    conn: &mut SqliteConnection,
    program_id: i64,
    name: &str,
    days: Weekdays,
) -> AppResult<i64> {
    Ok(sqlx::query_scalar(
        "INSERT INTO gym_routines (program_id, name, weekdays, position)
         VALUES (?1, ?2, ?3,
             (SELECT COALESCE(MAX(position) + 1, 0) FROM gym_routines WHERE program_id = ?1))
         RETURNING id",
    )
    .bind(program_id)
    .bind(name)
    .bind(days.bits())
    .fetch_one(&mut *conn)
    .await?)
}

pub async fn create_routine(
    db: &SqlitePool,
    program_id: i64,
    name: &str,
    days: Weekdays,
) -> AppResult<i64> {
    let mut conn = db.acquire().await?;
    insert_routine(&mut conn, program_id, name, days).await
}

pub async fn update_routine(
    db: &SqlitePool,
    id: i64,
    name: &str,
    days: Weekdays,
) -> AppResult<bool> {
    let res = sqlx::query(
        "UPDATE gym_routines SET name = ?, weekdays = ?,
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
         WHERE id = ?",
    )
    .bind(name)
    .bind(days.bits())
    .bind(id)
    .execute(db)
    .await?;
    Ok(res.rows_affected() > 0)
}

pub async fn delete_routine(db: &SqlitePool, id: i64) -> AppResult<bool> {
    let res = sqlx::query("DELETE FROM gym_routines WHERE id = ?")
        .bind(id)
        .execute(db)
        .await?;
    Ok(res.rows_affected() > 0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Move {
    Up,
    Down,
}

impl Move {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "up" => Some(Move::Up),
            "down" => Some(Move::Down),
            _ => None,
        }
    }
}

/// Moves `id` one place within `ids` (in order); `None` when it cannot move.
fn moved(mut ids: Vec<i64>, id: i64, dir: Move) -> Option<Vec<i64>> {
    let i = ids.iter().position(|x| *x == id)?;
    let j = match dir {
        Move::Up => i.checked_sub(1)?,
        Move::Down => Some(i + 1).filter(|j| *j < ids.len())?,
    };
    ids.swap(i, j);
    Some(ids)
}

/// Swaps an item with its neighbour and renumbers the list, so positions
/// stay dense whatever earlier deletions left behind. `table` and `parent`
/// are fixed identifiers chosen by the caller, never user input.
async fn reorder(
    db: &SqlitePool,
    table: &'static str,
    parent: &'static str,
    parent_id: i64,
    id: i64,
    dir: Move,
) -> AppResult<()> {
    let mut tx = db.begin().await?;
    let ids: Vec<i64> = sqlx::query_scalar(AssertSqlSafe(format!(
        "SELECT id FROM {table} WHERE {parent} = ? ORDER BY position, id"
    )))
    .bind(parent_id)
    .fetch_all(&mut *tx)
    .await?;
    if let Some(ids) = moved(ids, id, dir) {
        for (pos, rid) in ids.iter().enumerate() {
            sqlx::query(AssertSqlSafe(format!(
                "UPDATE {table} SET position = ? WHERE id = ?"
            )))
            .bind(pos as i64)
            .bind(rid)
            .execute(&mut *tx)
            .await?;
        }
    }
    tx.commit().await?;
    Ok(())
}

pub async fn move_routine(db: &SqlitePool, r: &Routine, dir: Move) -> AppResult<()> {
    reorder(db, "gym_routines", "program_id", r.program_id, r.id, dir).await
}

// ---------- Routine exercises ----------

pub async fn items(db: &SqlitePool, routine_id: i64) -> AppResult<Vec<RoutineExercise>> {
    Ok(sqlx::query_as::<_, RoutineExercise>(
        "SELECT id, routine_id, position, exercise_key, custom_exercise_id, sets, reps_min,
                reps_max, duration_s, weight_kg, rest_s, note
         FROM gym_routine_exercises WHERE routine_id = ? ORDER BY position, id",
    )
    .bind(routine_id)
    .fetch_all(db)
    .await?)
}

pub async fn item(db: &SqlitePool, id: i64) -> AppResult<Option<RoutineExercise>> {
    Ok(sqlx::query_as::<_, RoutineExercise>(
        "SELECT id, routine_id, position, exercise_key, custom_exercise_id, sets, reps_min,
                reps_max, duration_s, weight_kg, rest_s, note
         FROM gym_routine_exercises WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(db)
    .await?)
}

pub async fn item_count(db: &SqlitePool, routine_id: i64) -> AppResult<i64> {
    Ok(
        sqlx::query_scalar("SELECT COUNT(*) FROM gym_routine_exercises WHERE routine_id = ?")
            .bind(routine_id)
            .fetch_one(db)
            .await?,
    )
}

async fn insert_item(
    conn: &mut SqliteConnection,
    routine_id: i64,
    exercise: &ExerciseRef,
    v: &ItemInput,
) -> AppResult<i64> {
    let (key, custom) = match exercise {
        ExerciseRef::Builtin(e) => (Some(e.key), None),
        ExerciseRef::Custom(c) => (None, Some(c.id)),
    };
    let (reps_min, reps_max, duration_s) = v.reps();
    Ok(sqlx::query_scalar(
        "INSERT INTO gym_routine_exercises (routine_id, position, exercise_key,
             custom_exercise_id, sets, reps_min, reps_max, duration_s, weight_kg, rest_s, note)
         VALUES (?1, (SELECT COALESCE(MAX(position) + 1, 0) FROM gym_routine_exercises
                      WHERE routine_id = ?1),
                 ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         RETURNING id",
    )
    .bind(routine_id)
    .bind(key)
    .bind(custom)
    .bind(v.sets)
    .bind(reps_min)
    .bind(reps_max)
    .bind(duration_s)
    .bind(v.weight_kg)
    .bind(v.rest_s)
    .bind(&v.note)
    .fetch_one(&mut *conn)
    .await?)
}

pub async fn add_item(
    db: &SqlitePool,
    routine_id: i64,
    exercise: &ExerciseRef,
    v: &ItemInput,
) -> AppResult<i64> {
    let mut conn = db.acquire().await?;
    insert_item(&mut conn, routine_id, exercise, v).await
}

pub async fn update_item(db: &SqlitePool, id: i64, v: &ItemInput) -> AppResult<bool> {
    let (reps_min, reps_max, duration_s) = v.reps();
    let res = sqlx::query(
        "UPDATE gym_routine_exercises SET sets = ?, reps_min = ?, reps_max = ?, duration_s = ?,
             weight_kg = ?, rest_s = ?, note = ?,
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
         WHERE id = ?",
    )
    .bind(v.sets)
    .bind(reps_min)
    .bind(reps_max)
    .bind(duration_s)
    .bind(v.weight_kg)
    .bind(v.rest_s)
    .bind(&v.note)
    .bind(id)
    .execute(db)
    .await?;
    Ok(res.rows_affected() > 0)
}

pub async fn delete_item(db: &SqlitePool, id: i64) -> AppResult<bool> {
    let res = sqlx::query("DELETE FROM gym_routine_exercises WHERE id = ?")
        .bind(id)
        .execute(db)
        .await?;
    Ok(res.rows_affected() > 0)
}

pub async fn move_item(db: &SqlitePool, item: &RoutineExercise, dir: Move) -> AppResult<()> {
    reorder(
        db,
        "gym_routine_exercises",
        "routine_id",
        item.routine_id,
        item.id,
        dir,
    )
    .await
}

/// The exercise an item refers to; `None` for a key no longer in the list.
pub async fn exercise_of(
    db: &SqlitePool,
    item: &RoutineExercise,
) -> AppResult<Option<ExerciseRef>> {
    if let Some(key) = &item.exercise_key {
        return Ok(exercises::find(key).map(ExerciseRef::Builtin));
    }
    match item.custom_exercise_id {
        Some(id) => Ok(custom_exercise(db, id).await?.map(ExerciseRef::Custom)),
        None => Ok(None),
    }
}

// ---------- Custom exercises ----------

pub async fn custom_exercises(db: &SqlitePool) -> AppResult<Vec<CustomExercise>> {
    Ok(sqlx::query_as::<_, CustomExercise>(
        "SELECT id, name, muscle, equipment, timed FROM gym_exercises
         ORDER BY name COLLATE NOCASE, id LIMIT ?",
    )
    .bind(MAX_CUSTOM_EXERCISES)
    .fetch_all(db)
    .await?)
}

pub async fn custom_exercise(db: &SqlitePool, id: i64) -> AppResult<Option<CustomExercise>> {
    Ok(sqlx::query_as::<_, CustomExercise>(
        "SELECT id, name, muscle, equipment, timed FROM gym_exercises WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(db)
    .await?)
}

pub async fn custom_exercise_count(db: &SqlitePool) -> AppResult<i64> {
    Ok(sqlx::query_scalar("SELECT COUNT(*) FROM gym_exercises")
        .fetch_one(db)
        .await?)
}

pub struct CustomInput {
    pub name: String,
    pub muscle: Muscle,
    pub equipment: Equipment,
    pub timed: bool,
}

pub async fn create_custom_exercise(db: &SqlitePool, c: &CustomInput) -> AppResult<i64> {
    Ok(sqlx::query_scalar(
        "INSERT INTO gym_exercises (name, muscle, equipment, timed) VALUES (?, ?, ?, ?)
         RETURNING id",
    )
    .bind(&c.name)
    .bind(c.muscle.as_str())
    .bind(c.equipment.as_str())
    .bind(c.timed)
    .fetch_one(db)
    .await?)
}

/// Updates a custom exercise. Whether it is timed only changes while no
/// routine uses it: the routines' sets are counted the other way.
pub async fn update_custom_exercise(db: &SqlitePool, id: i64, c: &CustomInput) -> AppResult<bool> {
    let res = sqlx::query(
        "UPDATE gym_exercises SET name = ?, muscle = ?, equipment = ?,
             timed = CASE WHEN EXISTS (SELECT 1 FROM gym_routine_exercises
                                       WHERE custom_exercise_id = gym_exercises.id)
                          THEN timed ELSE ? END,
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
         WHERE id = ?",
    )
    .bind(&c.name)
    .bind(c.muscle.as_str())
    .bind(c.equipment.as_str())
    .bind(c.timed)
    .bind(id)
    .execute(db)
    .await?;
    Ok(res.rows_affected() > 0)
}

/// How many routine exercises use a custom exercise.
pub async fn custom_exercise_usage(db: &SqlitePool, id: i64) -> AppResult<i64> {
    Ok(sqlx::query_scalar(
        "SELECT COUNT(*) FROM gym_routine_exercises WHERE custom_exercise_id = ?",
    )
    .bind(id)
    .fetch_one(db)
    .await?)
}

#[derive(Debug, PartialEq)]
pub enum DeleteResult {
    Deleted,
    NotFound,
    InUse,
}

pub async fn delete_custom_exercise(db: &SqlitePool, id: i64) -> AppResult<DeleteResult> {
    if custom_exercise_usage(db, id).await? > 0 {
        return Ok(DeleteResult::InUse);
    }
    let res = sqlx::query("DELETE FROM gym_exercises WHERE id = ?")
        .bind(id)
        .execute(db)
        .await?;
    Ok(if res.rows_affected() > 0 {
        DeleteResult::Deleted
    } else {
        DeleteResult::NotFound
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reps_are_a_number_or_a_range() {
        assert_eq!(parse_reps(""), Ok(None));
        assert_eq!(parse_reps(" 8 "), Ok(Some((8, None))));
        assert_eq!(parse_reps("8-12"), Ok(Some((8, Some(12)))));
        assert_eq!(parse_reps("8 – 12"), Ok(Some((8, Some(12)))));
        assert_eq!(parse_reps("10-10"), Ok(Some((10, None))));
        assert_eq!(parse_reps("12-8"), Err(ParseError::Invalid));
        assert_eq!(parse_reps("0"), Err(ParseError::OutOfRange));
        assert_eq!(parse_reps("101"), Err(ParseError::OutOfRange));
        assert_eq!(parse_reps("-3"), Err(ParseError::Invalid));
        assert_eq!(parse_reps("8.5"), Err(ParseError::Invalid));
        assert_eq!(parse_reps("abc"), Err(ParseError::Invalid));
    }

    #[test]
    fn times_are_seconds_or_minutes_and_seconds() {
        assert_eq!(parse_seconds("", 900), Ok(None));
        assert_eq!(parse_seconds("90", 900), Ok(Some(90)));
        assert_eq!(parse_seconds("1:30", 900), Ok(Some(90)));
        assert_eq!(parse_seconds("0", 900), Ok(Some(0)));
        assert_eq!(parse_seconds("15:00", 900), Ok(Some(900)));
        assert_eq!(parse_seconds("15:01", 900), Err(ParseError::OutOfRange));
        assert_eq!(parse_seconds("1:75", 900), Err(ParseError::Invalid));
        assert_eq!(parse_seconds("1:5", 900), Err(ParseError::Invalid));
        assert_eq!(parse_seconds("-5", 900), Err(ParseError::Invalid));
        assert_eq!(parse_seconds("1.5", 900), Err(ParseError::Invalid));
    }

    #[test]
    fn times_and_loads_read_naturally() {
        assert_eq!(fmt_seconds(45), "45 s");
        assert_eq!(fmt_seconds(90), "1:30 min");
        assert_eq!(fmt_seconds(120), "2 min");
        assert_eq!(seconds_input(45), "45");
        assert_eq!(seconds_input(90), "1:30");
        assert_eq!(fmt_load(60.0, Lang::En), "60");
        assert_eq!(fmt_load(62.5, Lang::Fr), "62,5");
        assert_eq!(fmt_load(1.25, Lang::En), "1.25");
        assert_eq!(fmt_load(0.0, Lang::En), "0");
    }

    #[test]
    fn targets_name_reps_or_time() {
        let reps = |min, max| Target::Reps { min, max };
        assert_eq!(reps(8, None).label(Lang::En), "8 reps");
        assert_eq!(reps(1, None).label(Lang::En), "1 rep");
        assert_eq!(reps(8, Some(12)).label(Lang::Fr), "8–12 répétitions");
        assert_eq!(Target::Duration(60).label(Lang::Es), "1 min");
    }

    #[test]
    fn moving_stops_at_the_ends() {
        assert_eq!(moved(vec![1, 2, 3], 2, Move::Up), Some(vec![2, 1, 3]));
        assert_eq!(moved(vec![1, 2, 3], 2, Move::Down), Some(vec![1, 3, 2]));
        assert_eq!(moved(vec![1, 2, 3], 1, Move::Up), None);
        assert_eq!(moved(vec![1, 2, 3], 3, Move::Down), None);
        assert_eq!(moved(vec![1, 2, 3], 9, Move::Up), None);
    }
}
