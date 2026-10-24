//! Workouts: the sets actually done, logged from a routine or freely.
//!
//! A workout starts from a routine with its sets filled in from the last
//! time each exercise was done (or from the plan), is edited set by set and
//! is finished once. Finishing drops the sets that were not marked done.
//! The routine's plan is copied into the workout, so later changes to the
//! routine leave the history unchanged.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use sqlx::{AssertSqlSafe, SqliteConnection, SqlitePool};

use super::{ExerciseRef, MAX_SETS, Routine, RoutineExercise, exercises};
use crate::error::AppResult;
use crate::i18n::Lang;

pub const MAX_WORKOUT_EXERCISES: i64 = 40;
pub const MAX_SET_REPS: i64 = 1000;
pub const MAX_NOTE_CHARS: usize = 500;
/// Sets an exercise added without a plan or history starts with.
const DEFAULT_SETS: usize = 3;

#[derive(Clone, Debug, sqlx::FromRow, Serialize, Deserialize)]
pub struct Workout {
    pub id: i64,
    pub workout_date: String,
    pub name: String,
    pub routine_id: Option<i64>,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub note: Option<String>,
}

impl Workout {
    pub fn is_open(&self) -> bool {
        self.finished_at.is_none()
    }

    /// Whole minutes between start and finish, when both are known and the
    /// result is plausible (a workout logged afterwards has no real span).
    pub fn duration_min(&self) -> Option<i64> {
        let start: jiff::Timestamp = self.started_at.parse().ok()?;
        let end: jiff::Timestamp = self.finished_at.as_deref()?.parse().ok()?;
        let min = (end.as_second() - start.as_second()) / 60;
        (1..=24 * 60).contains(&min).then_some(min)
    }
}

#[derive(Clone, Debug, sqlx::FromRow, Serialize, Deserialize)]
pub struct WorkoutExercise {
    pub id: i64,
    pub workout_id: i64,
    pub position: i64,
    pub exercise_key: Option<String>,
    pub custom_exercise_id: Option<i64>,
    pub exercise_name: String,
    pub timed: bool,
    pub target_sets: Option<i64>,
    pub target_reps_min: Option<i64>,
    pub target_reps_max: Option<i64>,
    pub target_duration_s: Option<i64>,
    pub target_weight_kg: Option<f64>,
    pub rest_s: Option<i64>,
}

impl WorkoutExercise {
    /// The name in the interface language for a built-in exercise, else the
    /// name it was logged with.
    pub fn name(&self, lang: Lang) -> String {
        self.exercise_key
            .as_deref()
            .and_then(exercises::find)
            .map_or_else(|| self.exercise_name.clone(), |e| e.name(lang).into())
    }

    pub fn key(&self) -> Option<ExerciseKey> {
        match (&self.exercise_key, self.custom_exercise_id) {
            (Some(k), _) => Some(ExerciseKey::Builtin(k.clone())),
            (None, Some(id)) => Some(ExerciseKey::Custom(id)),
            _ => None,
        }
    }

    pub fn plan(&self) -> Option<Plan> {
        Some(Plan {
            sets: self.target_sets?,
            reps_min: self.target_reps_min,
            reps_max: self.target_reps_max,
            duration_s: self.target_duration_s,
            weight_kg: self.target_weight_kg,
        })
    }
}

#[derive(Clone, Debug, sqlx::FromRow, Serialize, Deserialize)]
pub struct WorkoutSet {
    pub id: i64,
    pub workout_exercise_id: i64,
    pub position: i64,
    pub reps: Option<i64>,
    pub duration_s: Option<i64>,
    pub weight_kg: Option<f64>,
    pub done: bool,
}

/// Identifies the same exercise across workouts.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ExerciseKey {
    Builtin(String),
    Custom(i64),
}

impl ExerciseKey {
    pub fn of(e: &ExerciseRef) -> Self {
        match e {
            ExerciseRef::Builtin(b) => ExerciseKey::Builtin(b.key.into()),
            ExerciseRef::Custom(c) => ExerciseKey::Custom(c.id),
        }
    }
}

/// What a routine asked for, copied into the workout.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Plan {
    pub sets: i64,
    pub reps_min: Option<i64>,
    pub reps_max: Option<i64>,
    pub duration_s: Option<i64>,
    pub weight_kg: Option<f64>,
}

impl Plan {
    pub fn of(item: &RoutineExercise) -> Self {
        Plan {
            sets: item.sets,
            reps_min: item.reps_min,
            reps_max: item.reps_max,
            duration_s: item.duration_s,
            weight_kg: item.weight_kg,
        }
    }
}

/// One set as done, without its identity.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Done {
    pub reps: Option<i64>,
    pub duration_s: Option<i64>,
    pub weight_kg: Option<f64>,
}

/// The sets a workout exercise starts with: as many as planned (or as last
/// time, or three), each with last time's numbers for that set, else the
/// plan's. Nothing is marked done.
pub fn prefill(plan: Option<&Plan>, last: &[Done], timed: bool) -> Vec<Done> {
    let count = plan
        .map(|p| p.sets as usize)
        .filter(|n| *n > 0)
        .unwrap_or(if last.is_empty() {
            DEFAULT_SETS
        } else {
            last.len()
        })
        .min(MAX_SETS as usize);
    (0..count)
        .map(|i| {
            let prev = last.get(i).or(last.last());
            Done {
                reps: if timed {
                    None
                } else {
                    prev.and_then(|d| d.reps).or(plan.and_then(|p| p.reps_min))
                },
                duration_s: if timed {
                    prev.and_then(|d| d.duration_s)
                        .or(plan.and_then(|p| p.duration_s))
                } else {
                    None
                },
                weight_kg: prev
                    .and_then(|d| d.weight_kg)
                    .or(plan.and_then(|p| p.weight_kg)),
            }
        })
        .collect()
}

/// Whether last time every planned set reached the top of the rep range
/// (or the planned reps), a common sign that the weight can go up.
pub fn reached_top(plan: Option<&Plan>, last: &[Done]) -> bool {
    let Some(p) = plan else { return false };
    let Some(top) = p.reps_max.or(p.reps_min) else {
        return false;
    };
    !last.is_empty()
        && last.len() as i64 >= p.sets
        && last.iter().all(|d| d.reps.is_some_and(|r| r >= top))
}

/// Total load moved: weight × reps over the sets done.
pub fn volume_kg(sets: &[Done]) -> f64 {
    sets.iter()
        .map(|d| d.weight_kg.unwrap_or(0.0) * d.reps.unwrap_or(0) as f64)
        .sum()
}

// ---------- Reading ----------

const WORKOUT_COLUMNS: &str = "id, workout_date, name, routine_id, started_at, finished_at, note";
const EXERCISE_COLUMNS: &str = "id, workout_id, position, exercise_key, custom_exercise_id, \
     exercise_name, timed, target_sets, target_reps_min, target_reps_max, target_duration_s, \
     target_weight_kg, rest_s";

pub async fn get(db: &SqlitePool, id: i64) -> AppResult<Option<Workout>> {
    Ok(sqlx::query_as::<_, Workout>(AssertSqlSafe(format!(
        "SELECT {WORKOUT_COLUMNS} FROM gym_workouts WHERE id = ?"
    )))
    .bind(id)
    .fetch_optional(db)
    .await?)
}

/// The workout in progress, if any.
pub async fn open(db: &SqlitePool) -> AppResult<Option<Workout>> {
    Ok(sqlx::query_as::<_, Workout>(AssertSqlSafe(format!(
        "SELECT {WORKOUT_COLUMNS} FROM gym_workouts WHERE finished_at IS NULL"
    )))
    .fetch_optional(db)
    .await?)
}

pub async fn exercises_of(db: &SqlitePool, workout_id: i64) -> AppResult<Vec<WorkoutExercise>> {
    Ok(sqlx::query_as::<_, WorkoutExercise>(AssertSqlSafe(format!(
        "SELECT {EXERCISE_COLUMNS} FROM gym_workout_exercises WHERE workout_id = ?
         ORDER BY position, id"
    )))
    .bind(workout_id)
    .fetch_all(db)
    .await?)
}

pub async fn exercise(db: &SqlitePool, id: i64) -> AppResult<Option<WorkoutExercise>> {
    Ok(sqlx::query_as::<_, WorkoutExercise>(AssertSqlSafe(format!(
        "SELECT {EXERCISE_COLUMNS} FROM gym_workout_exercises WHERE id = ?"
    )))
    .bind(id)
    .fetch_optional(db)
    .await?)
}

/// Every set of a workout, in exercise and set order.
pub async fn sets_of(db: &SqlitePool, workout_id: i64) -> AppResult<Vec<WorkoutSet>> {
    Ok(sqlx::query_as::<_, WorkoutSet>(
        "SELECT s.id, s.workout_exercise_id, s.position, s.reps, s.duration_s, s.weight_kg,
                s.done
         FROM gym_workout_sets s
         JOIN gym_workout_exercises e ON e.id = s.workout_exercise_id
         WHERE e.workout_id = ?
         ORDER BY e.position, e.id, s.position, s.id",
    )
    .bind(workout_id)
    .fetch_all(db)
    .await?)
}

/// The sets done the last time each exercise was done in a finished workout
/// before `before` (a workout's date and id; `None` for "ever").
pub async fn last_done(
    db: &SqlitePool,
    keys: &[ExerciseKey],
    before: Option<(&str, i64)>,
) -> AppResult<HashMap<ExerciseKey, Vec<Done>>> {
    let mut out = HashMap::new();
    let builtin: Vec<&str> = keys
        .iter()
        .filter_map(|k| match k {
            ExerciseKey::Builtin(s) => Some(s.as_str()),
            ExerciseKey::Custom(_) => None,
        })
        .collect();
    let custom: Vec<i64> = keys
        .iter()
        .filter_map(|k| match k {
            ExerciseKey::Custom(id) => Some(*id),
            ExerciseKey::Builtin(_) => None,
        })
        .collect();
    if builtin.is_empty() && custom.is_empty() {
        return Ok(out);
    }
    let marks = |n: usize| vec!["?"; n.max(1)].join(",");
    // The latest workout exercise with at least one done set, per exercise.
    let sql = format!(
        "WITH candidates AS (
             SELECT e.id, e.exercise_key, e.custom_exercise_id,
                    ROW_NUMBER() OVER (
                        PARTITION BY e.exercise_key, e.custom_exercise_id
                        ORDER BY w.workout_date DESC, w.id DESC, e.id DESC) AS rn
             FROM gym_workout_exercises e
             JOIN gym_workouts w ON w.id = e.workout_id
             WHERE w.finished_at IS NOT NULL
               AND (? IS NULL OR w.workout_date < ? OR (w.workout_date = ? AND w.id < ?))
               AND (e.exercise_key IN ({}) OR e.custom_exercise_id IN ({}))
               AND EXISTS (SELECT 1 FROM gym_workout_sets s
                           WHERE s.workout_exercise_id = e.id AND s.done = 1)
         )
         SELECT c.exercise_key, c.custom_exercise_id, s.reps, s.duration_s, s.weight_kg
         FROM candidates c JOIN gym_workout_sets s ON s.workout_exercise_id = c.id
         WHERE c.rn = 1 AND s.done = 1
         ORDER BY c.id, s.position, s.id",
        marks(builtin.len()),
        marks(custom.len()),
    );
    let (date, id) = before.unzip();
    let mut q = sqlx::query_as::<
        _,
        (
            Option<String>,
            Option<i64>,
            Option<i64>,
            Option<i64>,
            Option<f64>,
        ),
    >(AssertSqlSafe(sql))
    .bind(date)
    .bind(date)
    .bind(date)
    .bind(id);
    if builtin.is_empty() {
        q = q.bind(None::<String>);
    }
    for k in &builtin {
        q = q.bind(*k);
    }
    if custom.is_empty() {
        q = q.bind(None::<i64>);
    }
    for c in &custom {
        q = q.bind(*c);
    }
    for (key, custom_id, reps, duration_s, weight_kg) in q.fetch_all(db).await? {
        let k = match (key, custom_id) {
            (Some(k), _) => ExerciseKey::Builtin(k),
            (None, Some(c)) => ExerciseKey::Custom(c),
            _ => continue,
        };
        out.entry(k).or_insert_with(Vec::new).push(Done {
            reps,
            duration_s,
            weight_kg,
        });
    }
    Ok(out)
}

#[derive(sqlx::FromRow)]
pub struct WorkoutSummary {
    #[sqlx(flatten)]
    pub workout: Workout,
    pub exercises: i64,
    pub sets: i64,
    pub volume_kg: f64,
}

/// Finished workouts, most recent first.
pub async fn history(db: &SqlitePool, limit: i64, offset: i64) -> AppResult<Vec<WorkoutSummary>> {
    Ok(sqlx::query_as::<_, WorkoutSummary>(
        "SELECT w.id, w.workout_date, w.name, w.routine_id, w.started_at, w.finished_at, w.note,
                (SELECT COUNT(*) FROM gym_workout_exercises e WHERE e.workout_id = w.id)
                    AS exercises,
                (SELECT COUNT(*) FROM gym_workout_sets s
                 JOIN gym_workout_exercises e ON e.id = s.workout_exercise_id
                 WHERE e.workout_id = w.id AND s.done = 1) AS sets,
                (SELECT COALESCE(SUM(COALESCE(s.weight_kg, 0) * COALESCE(s.reps, 0)), 0.0)
                 FROM gym_workout_sets s
                 JOIN gym_workout_exercises e ON e.id = s.workout_exercise_id
                 WHERE e.workout_id = w.id AND s.done = 1) AS volume_kg
         FROM gym_workouts w WHERE w.finished_at IS NOT NULL
         ORDER BY w.workout_date DESC, w.id DESC LIMIT ? OFFSET ?",
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(db)
    .await?)
}

pub async fn finished_count(db: &SqlitePool) -> AppResult<i64> {
    Ok(
        sqlx::query_scalar("SELECT COUNT(*) FROM gym_workouts WHERE finished_at IS NOT NULL")
            .fetch_one(db)
            .await?,
    )
}

/// The routine of a program done most recently, to find the next one of a
/// rotation.
pub async fn last_routine_of_program(db: &SqlitePool, program_id: i64) -> AppResult<Option<i64>> {
    Ok(sqlx::query_scalar(
        "SELECT w.routine_id FROM gym_workouts w
         JOIN gym_routines r ON r.id = w.routine_id
         WHERE r.program_id = ? AND w.finished_at IS NOT NULL
         ORDER BY w.workout_date DESC, w.id DESC LIMIT 1",
    )
    .bind(program_id)
    .fetch_optional(db)
    .await?)
}

// ---------- Writing ----------

#[derive(Debug, PartialEq)]
pub enum Started {
    Created(i64),
    /// A workout is already in progress; it has to be finished or discarded first.
    AlreadyOpen(i64),
}

/// An exercise to start a workout with.
pub struct StartExercise {
    pub exercise: ExerciseRef,
    pub plan: Option<Plan>,
    pub rest_s: Option<i64>,
}

async fn insert_exercise(
    conn: &mut SqliteConnection,
    workout_id: i64,
    s: &StartExercise,
    last: &[Done],
    lang: Lang,
) -> AppResult<i64> {
    let (key, custom) = match &s.exercise {
        ExerciseRef::Builtin(e) => (Some(e.key), None),
        ExerciseRef::Custom(c) => (None, Some(c.id)),
    };
    let timed = s.exercise.timed();
    let plan = s.plan.as_ref();
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO gym_workout_exercises (workout_id, position, exercise_key,
             custom_exercise_id, exercise_name, timed, target_sets, target_reps_min,
             target_reps_max, target_duration_s, target_weight_kg, rest_s)
         VALUES (?1, (SELECT COALESCE(MAX(position) + 1, 0) FROM gym_workout_exercises
                      WHERE workout_id = ?1),
                 ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
         RETURNING id",
    )
    .bind(workout_id)
    .bind(key)
    .bind(custom)
    .bind(s.exercise.name(lang))
    .bind(timed)
    .bind(plan.map(|p| p.sets))
    .bind(plan.and_then(|p| p.reps_min))
    .bind(plan.and_then(|p| p.reps_max))
    .bind(plan.and_then(|p| p.duration_s))
    .bind(plan.and_then(|p| p.weight_kg))
    .bind(s.rest_s)
    .fetch_one(&mut *conn)
    .await?;
    for (pos, d) in prefill(plan, last, timed).iter().enumerate() {
        sqlx::query(
            "INSERT INTO gym_workout_sets (workout_exercise_id, position, reps, duration_s,
                 weight_kg)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(pos as i64)
        .bind(d.reps)
        .bind(d.duration_s)
        .bind(d.weight_kg)
        .execute(&mut *conn)
        .await?;
    }
    Ok(id)
}

/// Starts a workout dated `date`, from a routine's exercises or empty.
pub async fn start(
    db: &SqlitePool,
    date: &str,
    name: &str,
    routine: Option<&Routine>,
    list: &[StartExercise],
    lang: Lang,
) -> AppResult<Started> {
    if let Some(w) = open(db).await? {
        return Ok(Started::AlreadyOpen(w.id));
    }
    let keys: Vec<ExerciseKey> = list.iter().map(|s| ExerciseKey::of(&s.exercise)).collect();
    let last = last_done(db, &keys, None).await?;
    let mut tx = db.begin().await?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO gym_workouts (workout_date, name, routine_id, started_at)
         VALUES (?, ?, ?, ?) RETURNING id",
    )
    .bind(date)
    .bind(name)
    .bind(routine.map(|r| r.id))
    .bind(crate::db::now_utc())
    .fetch_one(&mut *tx)
    .await?;
    for (s, k) in list.iter().zip(&keys) {
        let prev = last.get(k).map(Vec::as_slice).unwrap_or(&[]);
        insert_exercise(&mut tx, id, s, prev, lang).await?;
    }
    tx.commit().await?;
    Ok(Started::Created(id))
}

/// Adds an exercise to a workout, with sets from the last time it was done.
/// Returns the new workout exercise's id.
pub async fn add_exercise(
    db: &SqlitePool,
    w: &Workout,
    exercise: ExerciseRef,
    lang: Lang,
) -> AppResult<i64> {
    let key = ExerciseKey::of(&exercise);
    let last = last_done(
        db,
        std::slice::from_ref(&key),
        Some((&w.workout_date, w.id)),
    )
    .await?;
    let s = StartExercise {
        exercise,
        plan: None,
        rest_s: None,
    };
    let prev = last.get(&key).map(Vec::as_slice).unwrap_or(&[]);
    let mut tx = db.begin().await?;
    let id = insert_exercise(&mut tx, w.id, &s, prev, lang).await?;
    tx.commit().await?;
    Ok(id)
}

pub async fn exercise_count(db: &SqlitePool, workout_id: i64) -> AppResult<i64> {
    Ok(
        sqlx::query_scalar("SELECT COUNT(*) FROM gym_workout_exercises WHERE workout_id = ?")
            .bind(workout_id)
            .fetch_one(db)
            .await?,
    )
}

/// New values for one set, as validated by the form.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SetValues {
    pub id: i64,
    pub reps: Option<i64>,
    pub duration_s: Option<i64>,
    pub weight_kg: Option<f64>,
    pub done: bool,
}

pub struct Details<'a> {
    pub date: &'a str,
    pub name: &'a str,
    pub note: Option<&'a str>,
}

/// Saves a workout's details and set values in one transaction. Sets that
/// do not belong to the workout are ignored.
pub async fn save(
    db: &SqlitePool,
    workout_id: i64,
    details: &Details<'_>,
    sets: &[SetValues],
) -> AppResult<()> {
    let mut tx = db.begin().await?;
    sqlx::query(
        "UPDATE gym_workouts SET workout_date = ?, name = ?, note = ?,
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
         WHERE id = ?",
    )
    .bind(details.date)
    .bind(details.name)
    .bind(details.note)
    .bind(workout_id)
    .execute(&mut *tx)
    .await?;
    for s in sets {
        sqlx::query(
            "UPDATE gym_workout_sets SET reps = ?, duration_s = ?, weight_kg = ?, done = ?
             WHERE id = ? AND workout_exercise_id IN
                 (SELECT id FROM gym_workout_exercises WHERE workout_id = ?)",
        )
        .bind(s.reps)
        .bind(s.duration_s)
        .bind(s.weight_kg)
        .bind(s.done)
        .bind(s.id)
        .bind(workout_id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Adds a set copying the values of the exercise's last set. It is marked
/// done in a finished workout, where every set kept was done. Returns false
/// when the exercise already has the most sets allowed.
pub async fn add_set(db: &SqlitePool, exercise_id: i64, done: bool) -> AppResult<bool> {
    let res = sqlx::query(
        "INSERT INTO gym_workout_sets (workout_exercise_id, position, reps, duration_s, weight_kg,
             done)
         SELECT ?1, COALESCE(MAX(position) + 1, 0),
                (SELECT reps FROM gym_workout_sets WHERE workout_exercise_id = ?1
                 ORDER BY position DESC, id DESC LIMIT 1),
                (SELECT duration_s FROM gym_workout_sets WHERE workout_exercise_id = ?1
                 ORDER BY position DESC, id DESC LIMIT 1),
                (SELECT weight_kg FROM gym_workout_sets WHERE workout_exercise_id = ?1
                 ORDER BY position DESC, id DESC LIMIT 1),
                ?3
         FROM gym_workout_sets WHERE workout_exercise_id = ?1
         HAVING COUNT(*) < ?2",
    )
    .bind(exercise_id)
    .bind(MAX_SETS)
    .bind(done)
    .execute(db)
    .await?;
    Ok(res.rows_affected() > 0)
}

/// Removes the exercise's last set, keeping at least one; removing the
/// exercise is the way to drop them all.
pub async fn remove_last_set(db: &SqlitePool, exercise_id: i64) -> AppResult<()> {
    sqlx::query(
        "DELETE FROM gym_workout_sets WHERE id =
             (SELECT id FROM gym_workout_sets WHERE workout_exercise_id = ?1
              ORDER BY position DESC, id DESC LIMIT 1)
           AND (SELECT COUNT(*) FROM gym_workout_sets WHERE workout_exercise_id = ?1) > 1",
    )
    .bind(exercise_id)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn remove_exercise(db: &SqlitePool, exercise_id: i64) -> AppResult<()> {
    sqlx::query("DELETE FROM gym_workout_exercises WHERE id = ?")
        .bind(exercise_id)
        .execute(db)
        .await?;
    Ok(())
}

#[derive(Debug, PartialEq)]
pub enum Finished {
    Finished,
    /// No set is marked done; nothing was changed.
    NothingDone,
}

/// Finishes a workout: undone sets and exercises left without sets are
/// dropped, and the end time is recorded.
pub async fn finish(db: &SqlitePool, workout_id: i64) -> AppResult<Finished> {
    let mut tx = db.begin().await?;
    let done: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM gym_workout_sets s
         JOIN gym_workout_exercises e ON e.id = s.workout_exercise_id
         WHERE e.workout_id = ? AND s.done = 1",
    )
    .bind(workout_id)
    .fetch_one(&mut *tx)
    .await?;
    if done == 0 {
        return Ok(Finished::NothingDone);
    }
    sqlx::query(
        "DELETE FROM gym_workout_sets WHERE done = 0 AND workout_exercise_id IN
             (SELECT id FROM gym_workout_exercises WHERE workout_id = ?)",
    )
    .bind(workout_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "DELETE FROM gym_workout_exercises WHERE workout_id = ? AND NOT EXISTS
             (SELECT 1 FROM gym_workout_sets s WHERE s.workout_exercise_id = gym_workout_exercises.id)",
    )
    .bind(workout_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE gym_workouts SET finished_at = ?,
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
         WHERE id = ? AND finished_at IS NULL",
    )
    .bind(crate::db::now_utc())
    .bind(workout_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Finished::Finished)
}

pub async fn delete(db: &SqlitePool, id: i64) -> AppResult<bool> {
    let res = sqlx::query("DELETE FROM gym_workouts WHERE id = ?")
        .bind(id)
        .execute(db)
        .await?;
    Ok(res.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn done(reps: i64, kg: f64) -> Done {
        Done {
            reps: Some(reps),
            duration_s: None,
            weight_kg: Some(kg),
        }
    }

    fn plan(sets: i64, min: i64, max: Option<i64>, kg: Option<f64>) -> Plan {
        Plan {
            sets,
            reps_min: Some(min),
            reps_max: max,
            duration_s: None,
            weight_kg: kg,
        }
    }

    #[test]
    fn sets_start_from_last_time_then_the_plan() {
        let p = plan(4, 8, Some(12), Some(60.0));
        // Nothing done before: the plan.
        let fresh = prefill(Some(&p), &[], false);
        assert_eq!(fresh.len(), 4);
        assert!(fresh.iter().all(|d| *d == done(8, 60.0)));
        // Last time's numbers set by set; a missing set repeats the last one.
        let last = [done(10, 62.5), done(9, 62.5), done(8, 60.0)];
        let next = prefill(Some(&p), &last, false);
        assert_eq!(
            next,
            [done(10, 62.5), done(9, 62.5), done(8, 60.0), done(8, 60.0)]
        );
        // Without a plan: as many sets as last time, or three empty ones.
        assert_eq!(prefill(None, &last, false).len(), 3);
        let empty = prefill(None, &[], false);
        assert_eq!(empty, [Done::default(); 3]);
    }

    #[test]
    fn timed_sets_take_a_time_instead_of_reps() {
        let p = Plan {
            sets: 2,
            duration_s: Some(45),
            ..Default::default()
        };
        let sets = prefill(Some(&p), &[], true);
        assert_eq!(sets.len(), 2);
        assert!(
            sets.iter()
                .all(|d| d.duration_s == Some(45) && d.reps.is_none())
        );
    }

    #[test]
    fn the_top_of_the_range_on_every_set_suggests_more_weight() {
        let p = plan(3, 8, Some(12), None);
        assert!(reached_top(Some(&p), &[done(12, 50.0); 3]));
        assert!(!reached_top(
            Some(&p),
            &[done(12, 50.0), done(12, 50.0), done(11, 50.0)]
        ));
        assert!(
            !reached_top(Some(&p), &[done(12, 50.0); 2]),
            "fewer sets than planned"
        );
        assert!(reached_top(
            Some(&plan(5, 5, None, None)),
            &[done(5, 100.0); 5]
        ));
        assert!(!reached_top(None, &[done(12, 50.0); 3]));
        assert!(!reached_top(Some(&p), &[]));
    }

    #[test]
    fn volume_counts_weight_times_reps() {
        assert_eq!(volume_kg(&[done(10, 50.0), done(8, 52.5)]), 920.0);
        assert_eq!(volume_kg(&[Done::default()]), 0.0);
    }

    #[test]
    fn duration_needs_a_plausible_span() {
        let mut w = Workout {
            id: 1,
            workout_date: "2026-10-24".into(),
            name: "Push".into(),
            routine_id: None,
            started_at: "2026-10-24T17:00:00Z".into(),
            finished_at: Some("2026-10-24T18:05:30Z".into()),
            note: None,
        };
        assert_eq!(w.duration_min(), Some(65));
        w.finished_at = Some("2026-10-27T18:00:00Z".into());
        assert_eq!(w.duration_min(), None);
        w.finished_at = None;
        assert_eq!(w.duration_min(), None);
    }

    #[tokio::test]
    async fn workouts_start_prefilled_finish_and_feed_the_next_one() {
        let db = crate::db::connect_memory().await.unwrap();
        let bench = exercises::find("barbell_bench_press").unwrap();
        let list = || {
            vec![StartExercise {
                exercise: ExerciseRef::Builtin(bench),
                plan: Some(plan(3, 8, Some(12), Some(60.0))),
                rest_s: Some(120),
            }]
        };
        let Started::Created(id) = start(&db, "2026-10-20", "Push", None, &list(), Lang::En)
            .await
            .unwrap()
        else {
            panic!("not started");
        };
        // Only one workout at a time.
        assert_eq!(
            start(&db, "2026-10-20", "Push", None, &list(), Lang::En)
                .await
                .unwrap(),
            Started::AlreadyOpen(id)
        );
        let sets = sets_of(&db, id).await.unwrap();
        assert_eq!(sets.len(), 3);
        assert_eq!(finish(&db, id).await.unwrap(), Finished::NothingDone);

        let w = get(&db, id).await.unwrap().unwrap();
        let values: Vec<SetValues> = sets
            .iter()
            .take(2)
            .map(|s| SetValues {
                id: s.id,
                reps: Some(12),
                duration_s: None,
                weight_kg: Some(62.5),
                done: true,
            })
            .collect();
        let details = Details {
            date: &w.workout_date,
            name: &w.name,
            note: None,
        };
        save(&db, id, &details, &values).await.unwrap();
        assert_eq!(finish(&db, id).await.unwrap(), Finished::Finished);
        assert_eq!(
            sets_of(&db, id).await.unwrap().len(),
            2,
            "undone set dropped"
        );
        assert!(open(&db).await.unwrap().is_none());

        let Started::Created(next) = start(&db, "2026-10-22", "Push", None, &list(), Lang::En)
            .await
            .unwrap()
        else {
            panic!("not started");
        };
        let sets = sets_of(&db, next).await.unwrap();
        assert_eq!(sets.len(), 3);
        assert!(
            sets.iter()
                .all(|s| s.reps == Some(12) && s.weight_kg == Some(62.5))
        );
        assert!(sets.iter().all(|s| !s.done));

        // Adding and removing sets.
        let ex = exercises_of(&db, next).await.unwrap();
        assert!(add_set(&db, ex[0].id, false).await.unwrap());
        assert_eq!(sets_of(&db, next).await.unwrap().len(), 4);
        for _ in 0..5 {
            remove_last_set(&db, ex[0].id).await.unwrap();
        }
        assert_eq!(
            sets_of(&db, next).await.unwrap().len(),
            1,
            "one set is kept"
        );

        let hist = history(&db, 10, 0).await.unwrap();
        assert_eq!(hist.len(), 1);
        assert_eq!((hist[0].sets, hist[0].volume_kg), (2, 1500.0));
    }
}
