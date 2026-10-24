//! Export, import and consistent SQLite backups.

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::activity::{self, ActivitySkip, DailySteps, Weekdays};
use crate::db::now_utc;
use crate::diary::{Entry, Meal, check_amount};
use crate::error::{AppError, AppResult};
use crate::foods::{Food, search_text};
use crate::goals::{Activity, Goal, GoalSettings, MIN_CUSTOM_KCAL, Sex};
use crate::gym::exercises::{Equipment, Muscle};
use crate::gym::workouts::{Workout, WorkoutExercise, WorkoutSet};
use crate::gym::{self, CustomExercise, Program, Routine, RoutineExercise, Schedule};
use crate::i18n::Lang;
use crate::nutrition::{Basis, Nutrient, check_value};
use crate::profile::Profile;
use crate::units::{UnitSystem, parse_date, valid_timezone};
use crate::weight::{WeightEntry, check_weight_kg};

pub const EXPORT_FORMAT: &str = "korpus-export";
pub const EXPORT_VERSION: u32 = 1;
/// Upper bound for uploaded import files.
pub const MAX_IMPORT_BYTES: usize = 32 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
pub struct Export {
    pub format: String,
    pub version: u32,
    pub exported_at: String,
    pub profile: Profile,
    pub goals: GoalSettings,
    pub foods: Vec<Food>,
    pub diary_entries: Vec<Entry>,
    pub weight_entries: Vec<WeightEntry>,
    /// Activity data; absent in exports made before activity existed.
    #[serde(default)]
    pub daily_steps: Vec<DailySteps>,
    #[serde(default)]
    pub activities: Vec<activity::Activity>,
    #[serde(default)]
    pub activity_skips: Vec<ActivitySkip>,
    /// Gym programs; absent in exports made before they existed.
    #[serde(default)]
    pub gym_exercises: Vec<CustomExercise>,
    #[serde(default)]
    pub gym_programs: Vec<Program>,
    #[serde(default)]
    pub gym_routines: Vec<Routine>,
    #[serde(default)]
    pub gym_routine_exercises: Vec<RoutineExercise>,
    /// Logged workouts; absent in exports made before they existed.
    #[serde(default)]
    pub gym_workouts: Vec<Workout>,
    #[serde(default)]
    pub gym_workout_exercises: Vec<WorkoutExercise>,
    #[serde(default)]
    pub gym_workout_sets: Vec<WorkoutSet>,
}

pub async fn export(db: &SqlitePool) -> AppResult<Export> {
    let foods = crate::foods::all_stored(db).await?;
    let diary_entries = sqlx::query_as::<_, Entry>(
        "SELECT id, diary_date, meal, food_id, amount, unit, food_name, food_brand, source,
                energy_kcal, protein_g, carbs_g, fat_g, fiber_g, sugars_g, sat_fat_g, salt_g
         FROM diary_entries ORDER BY diary_date, id",
    )
    .fetch_all(db)
    .await?;
    let weight_entries = sqlx::query_as::<_, WeightEntry>(
        "SELECT id, entry_date, weight_kg, note FROM weight_entries ORDER BY entry_date",
    )
    .fetch_all(db)
    .await?;
    let daily_steps = sqlx::query_as::<_, DailySteps>(
        "SELECT step_date, steps FROM daily_steps ORDER BY step_date",
    )
    .fetch_all(db)
    .await?;
    let activities = sqlx::query_as::<_, activity::Activity>(
        "SELECT id, sport, sport_key, intensity, duration_min, note, start_date, weekdays,
             end_date
         FROM activities ORDER BY id",
    )
    .fetch_all(db)
    .await?;
    let activity_skips = sqlx::query_as::<_, ActivitySkip>(
        "SELECT activity_id, skip_date FROM activity_skips ORDER BY activity_id, skip_date",
    )
    .fetch_all(db)
    .await?;
    let gym_exercises = sqlx::query_as::<_, CustomExercise>(
        "SELECT id, name, muscle, equipment, timed FROM gym_exercises ORDER BY id",
    )
    .fetch_all(db)
    .await?;
    let gym_programs = sqlx::query_as::<_, Program>(
        "SELECT id, name, schedule, active FROM gym_programs ORDER BY id",
    )
    .fetch_all(db)
    .await?;
    let gym_routines = sqlx::query_as::<_, Routine>(
        "SELECT id, program_id, name, position, weekdays FROM gym_routines ORDER BY id",
    )
    .fetch_all(db)
    .await?;
    let gym_routine_exercises = sqlx::query_as::<_, RoutineExercise>(
        "SELECT id, routine_id, position, exercise_key, custom_exercise_id, sets, reps_min,
                reps_max, duration_s, weight_kg, rest_s, note
         FROM gym_routine_exercises ORDER BY id",
    )
    .fetch_all(db)
    .await?;
    let gym_workouts = sqlx::query_as::<_, Workout>(
        "SELECT id, workout_date, name, routine_id, started_at, finished_at, note
         FROM gym_workouts ORDER BY workout_date, id",
    )
    .fetch_all(db)
    .await?;
    let gym_workout_exercises = sqlx::query_as::<_, WorkoutExercise>(
        "SELECT id, workout_id, position, exercise_key, custom_exercise_id, exercise_name, timed,
                target_sets, target_reps_min, target_reps_max, target_duration_s,
                target_weight_kg, rest_s
         FROM gym_workout_exercises ORDER BY id",
    )
    .fetch_all(db)
    .await?;
    let gym_workout_sets = sqlx::query_as::<_, WorkoutSet>(
        "SELECT id, workout_exercise_id, position, reps, duration_s, weight_kg, done
         FROM gym_workout_sets ORDER BY id",
    )
    .fetch_all(db)
    .await?;
    Ok(Export {
        format: EXPORT_FORMAT.into(),
        version: EXPORT_VERSION,
        exported_at: now_utc(),
        profile: crate::profile::load(db).await?,
        goals: crate::goals::load(db).await?,
        foods,
        diary_entries,
        weight_entries,
        daily_steps,
        activities,
        activity_skips,
        gym_exercises,
        gym_programs,
        gym_routines,
        gym_routine_exercises,
        gym_workouts,
        gym_workout_exercises,
        gym_workout_sets,
    })
}

/// Prevents spreadsheet applications from interpreting a cell as a formula.
fn csv_field(s: &str) -> String {
    let guarded = if s.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{s}")
    } else {
        s.to_string()
    };
    if guarded.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", guarded.replace('"', "\"\""))
    } else {
        guarded
    }
}

fn csv_num(v: Option<f64>) -> String {
    v.map(|v| format!("{v:.2}")).unwrap_or_default()
}

/// Diary entries as CSV, one row per entry, with nutrients for the amount eaten.
pub fn diary_csv(entries: &[Entry]) -> String {
    let mut out = String::from(
        "date,meal,food,brand,amount,unit,energy_kcal,protein_g,carbs_g,fat_g,fiber_g,sugars_g,saturated_fat_g,salt_g,source\n",
    );
    for e in entries {
        let n = e.nutrients();
        let row = [
            csv_field(&e.diary_date),
            csv_field(&e.meal),
            csv_field(&e.food_name),
            csv_field(e.food_brand.as_deref().unwrap_or("")),
            format!("{}", e.amount),
            csv_field(&e.unit),
            csv_num(n.energy_kcal),
            csv_num(n.protein_g),
            csv_num(n.carbs_g),
            csv_num(n.fat_g),
            csv_num(n.fiber_g),
            csv_num(n.sugars_g),
            csv_num(n.sat_fat_g),
            csv_num(n.salt_g),
            csv_field(&e.source),
        ];
        out.push_str(&row.join(","));
        out.push('\n');
    }
    out
}

/// Writes a consistent snapshot of the database with `VACUUM INTO`, which is
/// safe while the application keeps writing, and returns its bytes.
///
/// The copy holds the whole diary, so it is written inside a fresh directory
/// only this process's user can open: the shared temporary directory is
/// readable by every local user, and a predictable file name there could be
/// claimed in advance.
pub async fn sqlite_snapshot(db: &SqlitePool) -> AppResult<Vec<u8>> {
    let dir = std::env::temp_dir().join(format!(
        "korpus-backup-{}-{}",
        std::process::id(),
        jiff::Timestamp::now().as_nanosecond()
    ));
    private_dir(&dir).map_err(|e| AppError::Internal(format!("creating backup directory: {e}")))?;
    let path = dir.join("korpus.db");
    let result = async {
        write_snapshot(db, &path).await?;
        tokio::fs::read(&path)
            .await
            .map_err(|e| AppError::Internal(format!("reading backup: {e}")))
    }
    .await;
    if let Err(e) = tokio::fs::remove_dir_all(&dir).await {
        tracing::warn!(error = %e, "could not remove temporary backup file");
    }
    result
}

/// Writes a consistent copy of the database to `path`, which must not exist.
/// The file is readable only by its owner: it holds the diary and the
/// login's password hash.
///
/// Sessions are left out. Restoring an older backup must not bring back a
/// login that was ended since, such as the sessions a password change logged
/// out after a cookie was stolen.
pub async fn write_snapshot(db: &SqlitePool, path: &std::path::Path) -> AppResult<()> {
    use sqlx::{ConnectOptions, Connection};
    let path_s = path
        .to_str()
        .ok_or_else(|| AppError::Internal("backup path is not valid UTF-8".into()))?;
    // Created empty first (which `VACUUM INTO` accepts) so the permissions
    // apply from the start and an existing file is never overwritten.
    let mut file = std::fs::OpenOptions::new();
    file.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut file, 0o600);
    file.open(path)
        .map_err(|e| AppError::Internal(format!("creating {path_s}: {e}")))?;
    let result = async {
        sqlx::query("VACUUM INTO ?")
            .bind(path_s)
            .execute(db)
            .await?;
        let mut copy = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(path)
            .connect()
            .await?;
        sqlx::query("PRAGMA secure_delete = ON")
            .execute(&mut copy)
            .await?;
        sqlx::query("DELETE FROM sessions")
            .execute(&mut copy)
            .await?;
        copy.close().await?;
        Ok(())
    }
    .await;
    if result.is_err() {
        // Never leave a partial copy that looks like a backup.
        if let Err(e) = std::fs::remove_file(path) {
            tracing::warn!(error = %e, "could not remove incomplete backup");
        }
    }
    result
}

/// Creates a new directory readable only by its owner; fails if it exists.
fn private_dir(path: &std::path::Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(path)
}

#[derive(Debug, PartialEq)]
pub struct ImportSummary {
    pub foods: usize,
    pub diary_entries: usize,
    pub weight_entries: usize,
    pub activities: usize,
    pub gym_programs: usize,
    pub gym_workouts: usize,
}

fn invalid(lang: Lang, what: impl std::fmt::Display) -> AppError {
    AppError::BadRequest(lang.f("The import file is invalid: {0}.", &[&what]))
}

/// "<record>: <problem>", e.g. "food 3: name".
fn at(record: String, problem: impl std::fmt::Display) -> String {
    format!("{record}: {problem}")
}

fn check_nutrients(
    basis: Basis,
    values: [(Nutrient, Option<f64>); 8],
    ctx: &str,
    lang: Lang,
) -> AppResult<()> {
    for (n, v) in values {
        if let Some(v) = v {
            check_value(n, basis, v).map_err(|e| {
                invalid(
                    lang,
                    format!("{ctx}: {} {}", n.label(lang), e.message(lang)),
                )
            })?;
        }
    }
    Ok(())
}

/// Text longer than `max` characters. Bounds match the manual forms (or,
/// for values Korpus writes itself, leave generous room).
fn too_long(s: Option<&str>, max: usize) -> bool {
    s.is_some_and(|s| s.chars().count() > max)
}

/// Highest energy target or estimate accepted from an import.
const MAX_IMPORTED_KCAL: f64 = 20_000.0;

/// Goal settings as the goals page could have saved them. An imported file
/// must not install a target the page would refuse, such as a starvation
/// diet.
fn valid_goals(g: &GoalSettings) -> bool {
    let in_range = |v: Option<f64>, min: f64, max: f64| v.is_none_or(|v| (min..=max).contains(&v));
    let known = |v: Option<&str>, parse: fn(&str) -> bool| v.is_none_or(parse);
    in_range(g.daily_kcal_target, MIN_CUSTOM_KCAL, MAX_IMPORTED_KCAL)
        && in_range(g.maintenance_kcal, 1.0, MAX_IMPORTED_KCAL)
        && in_range(g.height_cm, 100.0, 250.0)
        && in_range(g.weight_kg, crate::weight::MIN_KG, crate::weight::MAX_KG)
        && [g.protein_goal_g, g.carbs_goal_g, g.fat_goal_g]
            .into_iter()
            .all(|v| in_range(v, 0.0, 1000.0))
        && known(g.target_source.as_deref(), |s| {
            matches!(s, "estimate" | "custom")
        })
        && known(g.macro_source.as_deref(), |s| {
            matches!(s, "estimate" | "custom")
        })
        && Goal::parse(&g.goal).is_some()
        && known(g.sex.as_deref(), |s| Sex::parse(s).is_some())
        && known(g.activity.as_deref(), |s| Activity::parse(s).is_some())
        && known(g.birth_date.as_deref(), |s| parse_date(s).is_some())
        && !too_long(Some(&g.updated_at), 32)
}

fn validate(export: &Export, lang: Lang) -> AppResult<()> {
    if export.format != EXPORT_FORMAT {
        return Err(invalid(lang, lang.t("this is not a Korpus export")));
    }
    if export.version != EXPORT_VERSION {
        return Err(invalid(
            lang,
            lang.f("unsupported export version {0}", &[&export.version]),
        ));
    }
    if UnitSystem::parse(&export.profile.unit_system).is_none() {
        return Err(invalid(lang, lang.t("unknown unit system")));
    }
    if !valid_timezone(&export.profile.timezone) {
        return Err(invalid(lang, lang.t("unknown time zone")));
    }
    if export.profile.language.is_some() && export.profile.lang().is_none() {
        return Err(invalid(lang, lang.t("unknown language")));
    }
    if export.profile.country.is_some() && export.profile.country().is_none() {
        return Err(invalid(lang, lang.t("unknown country")));
    }
    for f in &export.foods {
        let food = || lang.f("food {0}", &[&f.id]);
        let basis =
            Basis::parse(&f.basis).ok_or_else(|| invalid(lang, at(food(), lang.t("basis"))))?;
        let bad_name = |n: &str| n.trim().is_empty() || n.chars().count() > 200;
        if bad_name(&f.name) {
            return Err(invalid(lang, at(food(), lang.t("name"))));
        }
        for n in &f.names {
            if Lang::parse(&n.lang).is_none() || bad_name(&n.name) {
                return Err(invalid(lang, at(food(), lang.t("name"))));
            }
        }
        if too_long(f.brand.as_deref(), 120)
            || too_long(f.barcode.as_deref(), 32)
            || too_long(f.serving_label.as_deref(), 60)
            || too_long(f.notes.as_deref(), 4000)
            || too_long(Some(&f.source), 32)
            || too_long(f.provider_id.as_deref(), 64)
            || [
                f.source_updated_at.as_deref(),
                f.fetched_at.as_deref(),
                Some(&f.created_at),
                Some(&f.updated_at),
            ]
            .into_iter()
            .any(|t| too_long(t, 32))
        {
            return Err(invalid(lang, at(food(), lang.t("text too long"))));
        }
        let n = f.nutrients();
        check_nutrients(basis, Nutrient::ALL.map(|k| (k, n.get(k))), &food(), lang)?;
    }
    for e in &export.diary_entries {
        let ctx = lang.f("diary entry {0}", &[&e.id]);
        parse_date(&e.diary_date).ok_or_else(|| invalid(lang, at(ctx.clone(), lang.t("date"))))?;
        Meal::parse(&e.meal).ok_or_else(|| invalid(lang, at(ctx.clone(), lang.t("meal"))))?;
        let basis =
            Basis::parse(&e.unit).ok_or_else(|| invalid(lang, at(ctx.clone(), lang.t("unit"))))?;
        check_amount(e.amount, lang).map_err(|m| invalid(lang, at(ctx.clone(), m)))?;
        if e.food_name.trim().is_empty() || too_long(Some(&e.food_name), 200) {
            return Err(invalid(lang, at(ctx, lang.t("name"))));
        }
        if too_long(e.food_brand.as_deref(), 120) || too_long(Some(&e.source), 32) {
            return Err(invalid(lang, at(ctx, lang.t("text too long"))));
        }
        let n = e.per_100();
        check_nutrients(basis, Nutrient::ALL.map(|k| (k, n.get(k))), &ctx, lang)?;
    }
    for w in &export.weight_entries {
        let ctx = || lang.f("weight {0}", &[&w.id]);
        parse_date(&w.entry_date).ok_or_else(|| invalid(lang, at(ctx(), lang.t("date"))))?;
        check_weight_kg(w.weight_kg, lang).map_err(|m| invalid(lang, at(ctx(), m)))?;
        if too_long(w.note.as_deref(), 500) {
            return Err(invalid(lang, at(ctx(), lang.t("text too long"))));
        }
    }
    if !(activity::MIN_STEP_GOAL..=activity::MAX_STEPS).contains(&export.profile.step_goal) {
        return Err(invalid(lang, lang.t("step goal")));
    }
    for s in &export.daily_steps {
        let ctx = || lang.f("steps for {0}", &[&s.step_date]);
        parse_date(&s.step_date).ok_or_else(|| invalid(lang, at(ctx(), lang.t("date"))))?;
        if !(0..=activity::MAX_STEPS).contains(&s.steps) {
            return Err(invalid(lang, at(ctx(), lang.t("steps"))));
        }
    }
    for a in &export.activities {
        let ctx = || lang.f("activity {0}", &[&a.id]);
        if a.sport.trim().is_empty() || too_long(Some(&a.sport), activity::MAX_SPORT_CHARS) {
            return Err(invalid(lang, at(ctx(), lang.t("name"))));
        }
        if a.sport_key.is_some() && a.listed_sport().is_none() {
            return Err(invalid(lang, at(ctx(), lang.t("sport"))));
        }
        if a.intensity.is_some() && (a.intensity().is_none() || a.sport_key.is_some()) {
            return Err(invalid(lang, at(ctx(), lang.t("intensity"))));
        }
        if too_long(a.note.as_deref(), activity::MAX_NOTE_CHARS) {
            return Err(invalid(lang, at(ctx(), lang.t("text too long"))));
        }
        if !(1..=activity::MAX_DURATION_MIN).contains(&a.duration_min) {
            return Err(invalid(lang, at(ctx(), lang.t("duration"))));
        }
        let start =
            parse_date(&a.start_date).ok_or_else(|| invalid(lang, at(ctx(), lang.t("date"))))?;
        let repeat = Weekdays::from_bits(a.weekdays)
            .ok_or_else(|| invalid(lang, at(ctx(), lang.t("days of the week"))))?;
        if let Some(end) = &a.end_date {
            let end_ok = parse_date(end).is_some_and(|end| end >= start) && !repeat.is_empty();
            if !end_ok {
                return Err(invalid(lang, at(ctx(), lang.t("date"))));
            }
        }
    }
    for s in &export.activity_skips {
        let ctx = || lang.f("activity {0}", &[&s.activity_id]);
        if !export.activities.iter().any(|a| a.id == s.activity_id) {
            return Err(invalid(lang, at(ctx(), lang.t("unknown activity"))));
        }
        parse_date(&s.skip_date).ok_or_else(|| invalid(lang, at(ctx(), lang.t("date"))))?;
    }
    if !valid_goals(&export.goals) {
        return Err(invalid(lang, lang.t("goal settings")));
    }
    validate_gym(export, lang)
}

/// Gym data as the gym pages could have saved it.
fn validate_gym(export: &Export, lang: Lang) -> AppResult<()> {
    let bad_name = |n: &str| n.trim().is_empty() || n.chars().count() > gym::MAX_NAME_CHARS;
    let too_many = |n: usize, max: i64| n > usize::try_from(max).unwrap_or(usize::MAX);
    if too_many(export.gym_exercises.len(), gym::MAX_CUSTOM_EXERCISES)
        || too_many(export.gym_programs.len(), gym::MAX_PROGRAMS)
        || too_many(
            export.gym_routines.len(),
            gym::MAX_PROGRAMS * gym::MAX_ROUTINES,
        )
        || too_many(
            export.gym_routine_exercises.len(),
            gym::MAX_PROGRAMS * gym::MAX_ROUTINES * gym::MAX_ITEMS,
        )
    {
        return Err(invalid(lang, lang.t("too many gym records")));
    }
    for c in &export.gym_exercises {
        let ctx = || lang.f("exercise {0}", &[&c.id]);
        if bad_name(&c.name)
            || Muscle::parse(&c.muscle).is_none()
            || Equipment::parse(&c.equipment).is_none()
        {
            return Err(invalid(lang, at(ctx(), lang.t("name"))));
        }
    }
    if export.gym_programs.iter().filter(|p| p.active).count() > 1 {
        return Err(invalid(lang, lang.t("more than one current program")));
    }
    for p in &export.gym_programs {
        if bad_name(&p.name) || Schedule::parse(&p.schedule).is_none() {
            return Err(invalid(
                lang,
                at(lang.f("program {0}", &[&p.id]), lang.t("name")),
            ));
        }
    }
    for r in &export.gym_routines {
        let ctx = || lang.f("routine {0}", &[&r.id]);
        if !export.gym_programs.iter().any(|p| p.id == r.program_id) {
            return Err(invalid(lang, at(ctx(), lang.t("unknown program"))));
        }
        if bad_name(&r.name) || Weekdays::from_bits(r.weekdays).is_none() {
            return Err(invalid(lang, at(ctx(), lang.t("name"))));
        }
    }
    for x in &export.gym_routine_exercises {
        let ctx = || lang.f("routine exercise {0}", &[&x.id]);
        if !export.gym_routines.iter().any(|r| r.id == x.routine_id) {
            return Err(invalid(lang, at(ctx(), lang.t("unknown routine"))));
        }
        let exercise_ok = match (&x.exercise_key, x.custom_exercise_id) {
            (Some(key), None) => gym::exercises::find(key).is_some(),
            (None, Some(id)) => export.gym_exercises.iter().any(|c| c.id == id),
            _ => false,
        };
        if !exercise_ok {
            return Err(invalid(lang, at(ctx(), lang.t("unknown exercise"))));
        }
        let target_ok = match x.target() {
            Some(gym::Target::Reps { min, max }) => {
                (1..=gym::MAX_REPS).contains(&min)
                    && max.is_none_or(|m| m > min && m <= gym::MAX_REPS)
            }
            Some(gym::Target::Duration(s)) => (1..=gym::MAX_DURATION_S).contains(&s),
            None => false,
        };
        let plan_ok = target_ok
            && (1..=gym::MAX_SETS).contains(&x.sets)
            && x.weight_kg
                .is_none_or(|w| (0.0..=gym::MAX_LOAD_KG).contains(&w))
            && x.rest_s.is_none_or(|r| (0..=gym::MAX_REST_S).contains(&r))
            && !too_long(x.note.as_deref(), gym::MAX_NOTE_CHARS);
        if !plan_ok {
            return Err(invalid(
                lang,
                at(ctx(), lang.t("sets, reps, weight or rest")),
            ));
        }
    }
    validate_workouts(export, lang)
}

/// Bounds a lifetime of workouts while refusing absurd files.
const MAX_IMPORT_WORKOUTS: usize = 20_000;

/// Logged workouts as the workout pages could have saved them.
fn validate_workouts(export: &Export, lang: Lang) -> AppResult<()> {
    use std::collections::HashMap;

    let bad_name = |n: &str| n.trim().is_empty() || n.chars().count() > gym::MAX_NAME_CHARS;
    let max_exercises = MAX_IMPORT_WORKOUTS * gym::workouts::MAX_WORKOUT_EXERCISES as usize;
    if export.gym_workouts.len() > MAX_IMPORT_WORKOUTS
        || export.gym_workout_exercises.len() > max_exercises
        || export.gym_workout_sets.len() > max_exercises * gym::MAX_SETS as usize
    {
        return Err(invalid(lang, lang.t("too many gym records")));
    }
    if export.gym_workouts.iter().filter(|w| w.is_open()).count() > 1 {
        return Err(invalid(lang, lang.t("more than one workout in progress")));
    }
    let workouts: HashMap<i64, &Workout> = export.gym_workouts.iter().map(|w| (w.id, w)).collect();
    for w in &export.gym_workouts {
        let ctx = || lang.f("workout {0}", &[&w.id]);
        let ok = parse_date(&w.workout_date).is_some()
            && !bad_name(&w.name)
            && !w.started_at.trim().is_empty()
            && w.routine_id
                .is_none_or(|r| export.gym_routines.iter().any(|x| x.id == r))
            && !too_long(w.note.as_deref(), gym::workouts::MAX_NOTE_CHARS);
        if !ok {
            return Err(invalid(lang, at(ctx(), lang.t("date, name or routine"))));
        }
    }
    let mut exercises = HashMap::new();
    for x in &export.gym_workout_exercises {
        let ctx = || lang.f("workout exercise {0}", &[&x.id]);
        if !workouts.contains_key(&x.workout_id) {
            return Err(invalid(lang, at(ctx(), lang.t("unknown workout"))));
        }
        let exercise_ok = match (&x.exercise_key, x.custom_exercise_id) {
            (Some(key), None) => gym::exercises::find(key).is_some(),
            (None, Some(id)) => export.gym_exercises.iter().any(|c| c.id == id),
            // A deleted exercise of the user's, kept by name.
            (None, None) => true,
            (Some(_), Some(_)) => false,
        };
        if !exercise_ok || bad_name(&x.exercise_name) {
            return Err(invalid(lang, at(ctx(), lang.t("unknown exercise"))));
        }
        let in_range = |v: Option<i64>, max: i64| v.is_none_or(|v| (1..=max).contains(&v));
        let plan_ok = in_range(x.target_sets, gym::MAX_SETS)
            && in_range(x.target_reps_min, gym::MAX_REPS)
            && in_range(x.target_reps_max, gym::MAX_REPS)
            && in_range(x.target_duration_s, gym::MAX_DURATION_S)
            && x.target_weight_kg
                .is_none_or(|w| (0.0..=gym::MAX_LOAD_KG).contains(&w))
            && x.rest_s.is_none_or(|r| (0..=gym::MAX_REST_S).contains(&r));
        if !plan_ok {
            return Err(invalid(
                lang,
                at(ctx(), lang.t("sets, reps, weight or rest")),
            ));
        }
        exercises.insert(x.id, x.workout_id);
    }
    for s in &export.gym_workout_sets {
        let ctx = || lang.f("workout set {0}", &[&s.id]);
        if !exercises.contains_key(&s.workout_exercise_id) {
            return Err(invalid(lang, at(ctx(), lang.t("unknown workout exercise"))));
        }
        let ok = s
            .reps
            .is_none_or(|r| (0..=gym::workouts::MAX_SET_REPS).contains(&r))
            && s.duration_s
                .is_none_or(|d| (0..=gym::MAX_DURATION_S).contains(&d))
            && s.weight_kg
                .is_none_or(|w| (0.0..=gym::MAX_LOAD_KG).contains(&w));
        if !ok {
            return Err(invalid(lang, at(ctx(), lang.t("reps, time or weight"))));
        }
    }
    Ok(())
}

/// Restores an export into an installation that has no foods, diary entries
/// or weights yet. Refusing to merge into existing data avoids duplicates
/// and ambiguous conflicts. Everything happens in one transaction.
pub async fn import(db: &SqlitePool, export: &Export, lang: Lang) -> AppResult<ImportSummary> {
    validate(export, lang)?;

    let mut tx = db.begin().await?;
    let existing: i64 = sqlx::query_scalar(
        "SELECT (SELECT COUNT(*) FROM foods) + (SELECT COUNT(*) FROM diary_entries)
              + (SELECT COUNT(*) FROM weight_entries) + (SELECT COUNT(*) FROM activities)
              + (SELECT COUNT(*) FROM daily_steps) + (SELECT COUNT(*) FROM gym_programs)
              + (SELECT COUNT(*) FROM gym_exercises) + (SELECT COUNT(*) FROM gym_workouts)",
    )
    .fetch_one(&mut *tx)
    .await?;
    if existing > 0 {
        return Err(AppError::BadRequest(
            lang.t("Import is only possible into an empty installation. This one already contains foods, diary entries, weights, activity or gym programs.")
                .into(),
        ));
    }

    let p = &export.profile;
    sqlx::query(
        "UPDATE profile SET setup_complete = 1, unit_system = ?, timezone = ?, language = ?,
             country = ?, step_goal = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
         WHERE id = 1",
    )
    .bind(&p.unit_system)
    .bind(&p.timezone)
    .bind(p.lang().map(Lang::code))
    .bind(p.country())
    .bind(p.step_goal)
    .execute(&mut *tx)
    .await?;

    let g = &export.goals;
    sqlx::query(
        "UPDATE goal_settings SET daily_kcal_target = ?, target_source = ?, goal = ?, sex = ?,
             birth_date = ?, height_cm = ?, weight_kg = ?, activity = ?, special_circumstances = ?,
             maintenance_kcal = ?, calculation_version = ?, protein_goal_g = ?, carbs_goal_g = ?,
             fat_goal_g = ?, macro_source = ?, updated_at = ? WHERE id = 1",
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
    .bind(&g.macro_source)
    .bind(&g.updated_at)
    .execute(&mut *tx)
    .await?;

    // Ids are preserved, so diary references stay valid without remapping.
    for f in &export.foods {
        let names: Vec<&str> = std::iter::once(f.name.as_str())
            .chain(f.names.iter().map(|n| n.name.as_str()))
            .collect();
        let text = search_text(&names, f.brand.as_deref(), f.barcode.as_deref());
        sqlx::query(
            "INSERT INTO foods (id, name, brand, barcode, basis, energy_kcal, protein_g, carbs_g,
                 fat_g, fiber_g, sugars_g, sat_fat_g, salt_g, energy_from_kj, serving_amount,
                 serving_label, notes, source, provider_id, source_updated_at, fetched_at,
                 edited_locally, search_text, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(f.id)
        .bind(&f.name)
        .bind(&f.brand)
        .bind(&f.barcode)
        .bind(&f.basis)
        .bind(f.energy_kcal)
        .bind(f.protein_g)
        .bind(f.carbs_g)
        .bind(f.fat_g)
        .bind(f.fiber_g)
        .bind(f.sugars_g)
        .bind(f.sat_fat_g)
        .bind(f.salt_g)
        .bind(f.energy_from_kj)
        .bind(f.serving_amount)
        .bind(&f.serving_label)
        .bind(&f.notes)
        .bind(&f.source)
        .bind(&f.provider_id)
        .bind(&f.source_updated_at)
        .bind(&f.fetched_at)
        .bind(f.edited_locally)
        .bind(text)
        .bind(&f.created_at)
        .bind(&f.updated_at)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            invalid(
                lang,
                lang.f("food {0} could not be stored ({1})", &[&f.id, &e]),
            )
        })?;
        for n in &f.names {
            sqlx::query("INSERT INTO food_names (food_id, lang, name) VALUES (?, ?, ?)")
                .bind(f.id)
                .bind(&n.lang)
                .bind(&n.name)
                .execute(&mut *tx)
                .await
                .map_err(|e| {
                    invalid(
                        lang,
                        lang.f("food {0} could not be stored ({1})", &[&f.id, &e]),
                    )
                })?;
        }
    }

    for e in &export.diary_entries {
        sqlx::query(
            "INSERT INTO diary_entries (diary_date, meal, food_id, amount, unit, food_name,
                 food_brand, source, energy_kcal, protein_g, carbs_g, fat_g, fiber_g, sugars_g,
                 sat_fat_g, salt_g)
             VALUES (?, ?, (SELECT id FROM foods WHERE id = ?), ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&e.diary_date)
        .bind(&e.meal)
        .bind(e.food_id)
        .bind(e.amount)
        .bind(&e.unit)
        .bind(&e.food_name)
        .bind(&e.food_brand)
        .bind(&e.source)
        .bind(e.energy_kcal)
        .bind(e.protein_g)
        .bind(e.carbs_g)
        .bind(e.fat_g)
        .bind(e.fiber_g)
        .bind(e.sugars_g)
        .bind(e.sat_fat_g)
        .bind(e.salt_g)
        .execute(&mut *tx)
        .await?;
    }

    for w in &export.weight_entries {
        sqlx::query("INSERT INTO weight_entries (entry_date, weight_kg, note) VALUES (?, ?, ?)")
            .bind(&w.entry_date)
            .bind(w.weight_kg)
            .bind(&w.note)
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                invalid(
                    lang,
                    lang.f(
                        "weight for {0} could not be stored ({1})",
                        &[&w.entry_date, &e],
                    ),
                )
            })?;
    }

    for s in &export.daily_steps {
        sqlx::query("INSERT INTO daily_steps (step_date, steps) VALUES (?, ?)")
            .bind(&s.step_date)
            .bind(s.steps)
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                invalid(
                    lang,
                    lang.f(
                        "steps for {0} could not be stored ({1})",
                        &[&s.step_date, &e],
                    ),
                )
            })?;
    }

    // Ids are preserved so that skipped days keep pointing at their activity.
    for a in &export.activities {
        sqlx::query(
            "INSERT INTO activities
                 (id, sport, sport_key, intensity, duration_min, note, start_date, weekdays,
                  end_date)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(a.id)
        .bind(&a.sport)
        .bind(&a.sport_key)
        .bind(&a.intensity)
        .bind(a.duration_min)
        .bind(&a.note)
        .bind(&a.start_date)
        .bind(a.weekdays)
        .bind(&a.end_date)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            invalid(
                lang,
                lang.f("activity {0} could not be stored ({1})", &[&a.id, &e]),
            )
        })?;
    }
    for s in &export.activity_skips {
        sqlx::query("INSERT OR IGNORE INTO activity_skips (activity_id, skip_date) VALUES (?, ?)")
            .bind(s.activity_id)
            .bind(&s.skip_date)
            .execute(&mut *tx)
            .await?;
    }

    // Ids are preserved so routines keep pointing at their program and
    // exercises.
    for c in &export.gym_exercises {
        sqlx::query(
            "INSERT INTO gym_exercises (id, name, muscle, equipment, timed) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(c.id)
        .bind(&c.name)
        .bind(&c.muscle)
        .bind(&c.equipment)
        .bind(c.timed)
        .execute(&mut *tx)
        .await?;
    }
    for p in &export.gym_programs {
        sqlx::query("INSERT INTO gym_programs (id, name, schedule, active) VALUES (?, ?, ?, ?)")
            .bind(p.id)
            .bind(&p.name)
            .bind(&p.schedule)
            .bind(p.active)
            .execute(&mut *tx)
            .await?;
    }
    for r in &export.gym_routines {
        sqlx::query(
            "INSERT INTO gym_routines (id, program_id, name, position, weekdays)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(r.id)
        .bind(r.program_id)
        .bind(&r.name)
        .bind(r.position)
        .bind(r.weekdays)
        .execute(&mut *tx)
        .await?;
    }
    for x in &export.gym_routine_exercises {
        sqlx::query(
            "INSERT INTO gym_routine_exercises (id, routine_id, position, exercise_key,
                 custom_exercise_id, sets, reps_min, reps_max, duration_s, weight_kg, rest_s, note)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(x.id)
        .bind(x.routine_id)
        .bind(x.position)
        .bind(&x.exercise_key)
        .bind(x.custom_exercise_id)
        .bind(x.sets)
        .bind(x.reps_min)
        .bind(x.reps_max)
        .bind(x.duration_s)
        .bind(x.weight_kg)
        .bind(x.rest_s)
        .bind(&x.note)
        .execute(&mut *tx)
        .await?;
    }
    for w in &export.gym_workouts {
        sqlx::query(
            "INSERT INTO gym_workouts (id, workout_date, name, routine_id, started_at, finished_at,
                 note)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(w.id)
        .bind(&w.workout_date)
        .bind(&w.name)
        .bind(w.routine_id)
        .bind(&w.started_at)
        .bind(&w.finished_at)
        .bind(&w.note)
        .execute(&mut *tx)
        .await?;
    }
    for x in &export.gym_workout_exercises {
        sqlx::query(
            "INSERT INTO gym_workout_exercises (id, workout_id, position, exercise_key,
                 custom_exercise_id, exercise_name, timed, target_sets, target_reps_min,
                 target_reps_max, target_duration_s, target_weight_kg, rest_s)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(x.id)
        .bind(x.workout_id)
        .bind(x.position)
        .bind(&x.exercise_key)
        .bind(x.custom_exercise_id)
        .bind(&x.exercise_name)
        .bind(x.timed)
        .bind(x.target_sets)
        .bind(x.target_reps_min)
        .bind(x.target_reps_max)
        .bind(x.target_duration_s)
        .bind(x.target_weight_kg)
        .bind(x.rest_s)
        .execute(&mut *tx)
        .await?;
    }
    for st in &export.gym_workout_sets {
        sqlx::query(
            "INSERT INTO gym_workout_sets (id, workout_exercise_id, position, reps, duration_s,
                 weight_kg, done)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(st.id)
        .bind(st.workout_exercise_id)
        .bind(st.position)
        .bind(st.reps)
        .bind(st.duration_s)
        .bind(st.weight_kg)
        .bind(st.done)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(ImportSummary {
        foods: export.foods.len(),
        diary_entries: export.diary_entries.len(),
        weight_entries: export.weight_entries.len(),
        activities: export.activities.len(),
        gym_programs: export.gym_programs.len(),
        gym_workouts: export.gym_workouts.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn snapshots_leave_out_sessions() {
        // A file, not an in-memory database: `VACUUM INTO` from memory
        // writes the copy to memory too.
        let dir = std::env::temp_dir().join(format!(
            "korpus-snapshot-test-{}-{}",
            std::process::id(),
            jiff::Timestamp::now().as_nanosecond()
        ));
        private_dir(&dir).unwrap();
        let db = crate::db::connect(&format!("sqlite://{}", dir.join("live.db").display()))
            .await
            .unwrap();
        crate::auth::create_account(&db, "me", "correct horse battery")
            .await
            .unwrap();
        crate::auth::create_session(&db, false).await.unwrap();
        let path = dir.join("copy.db");
        write_snapshot(&db, &path).await.unwrap();
        assert!(
            write_snapshot(&db, &path).await.is_err(),
            "never overwrites a file"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o077, 0, "only the owner can read it");
        }
        let copy = crate::db::connect(&format!("sqlite://{}", path.display()))
            .await
            .unwrap();
        let sessions: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions")
            .fetch_one(&copy)
            .await
            .unwrap();
        let accounts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM account")
            .fetch_one(&copy)
            .await
            .unwrap();
        copy.close().await;
        db.close().await;
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!((sessions, accounts), (0, 1));
    }

    #[test]
    fn imported_goals_must_be_what_the_goals_page_allows() {
        let ok = GoalSettings {
            goal: "maintain".into(),
            daily_kcal_target: Some(2000.0),
            target_source: Some("custom".into()),
            ..Default::default()
        };
        assert!(valid_goals(&ok));
        assert!(valid_goals(&GoalSettings {
            goal: "lose".into(),
            ..Default::default()
        }));
        for bad in [
            GoalSettings {
                daily_kcal_target: Some(300.0),
                ..ok.clone()
            },
            GoalSettings {
                goal: "starve".into(),
                ..ok.clone()
            },
            GoalSettings {
                birth_date: Some("yesterday".into()),
                ..ok.clone()
            },
            GoalSettings {
                height_cm: Some(1.0),
                ..ok.clone()
            },
            GoalSettings {
                protein_goal_g: Some(-1.0),
                ..ok.clone()
            },
        ] {
            assert!(!valid_goals(&bad), "{bad:?}");
        }
    }

    #[test]
    fn csv_quotes_and_guards_formulas() {
        assert_eq!(csv_field("Oats"), "Oats");
        assert_eq!(csv_field("Oats, rolled"), "\"Oats, rolled\"");
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_field("=SUM(A1)"), "'=SUM(A1)");
        assert_eq!(csv_field("-1"), "'-1");
    }
}
