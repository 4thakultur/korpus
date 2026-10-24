//! Physical activity entered by hand: steps walked each day and sport
//! sessions, once or repeating every week.
//!
//! Steps and sport sessions are turned into approximate energy estimates
//! (see [`walking_kcal`] and [`crate::sports::net_kcal`]) that the diary
//! adds to that day's calorie target.

use jiff::civil::Date;
use sqlx::SqlitePool;

use crate::error::AppResult;
use crate::i18n::Lang;
use crate::sports::{self, Intensity, Sport};
use crate::units::{parse_date, weekday_short};

pub const MAX_STEPS: i64 = 100_000;
/// Bounds and default of the daily step goal, which fills the steps ring.
pub const MIN_STEP_GOAL: i64 = 1_000;
pub const DEFAULT_STEP_GOAL: i64 = 10_000;
/// Step length as a fraction of body height, a common pedometer estimate.
pub const STRIDE_PER_HEIGHT: f64 = 0.414;
/// Net energy cost of level walking, above resting: the ACSM walking
/// equation's 0.1 ml O2 per kg per metre, at about 5 kcal per litre of O2.
pub const NET_KCAL_PER_KG_KM: f64 = 0.5;
pub const MAX_DURATION_MIN: i64 = 24 * 60;
pub const MAX_SPORT_CHARS: usize = 80;
pub const MAX_NOTE_CHARS: usize = 500;

/// Days of the week an activity repeats on: bit 0 is Monday, bit 6 Sunday.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Weekdays(u8);

impl Weekdays {
    pub const NONE: Weekdays = Weekdays(0);
    pub const ALL: Weekdays = Weekdays(0x7f);

    pub fn from_bits(bits: i64) -> Option<Self> {
        u8::try_from(bits)
            .ok()
            .filter(|b| *b <= Self::ALL.0)
            .map(Weekdays)
    }

    pub fn bits(self) -> i64 {
        i64::from(self.0)
    }

    /// 0 for Monday … 6 for Sunday.
    pub fn index_of(d: Date) -> usize {
        usize::from(d.weekday().to_monday_zero_offset() as u8)
    }

    pub fn of(d: Date) -> Self {
        Weekdays::NONE.with(Self::index_of(d))
    }

    pub fn with(self, index: usize) -> Self {
        if index < 7 {
            Weekdays(self.0 | 1 << index)
        } else {
            self
        }
    }

    pub fn has(self, index: usize) -> bool {
        index < 7 && self.0 & (1 << index) != 0
    }

    pub fn contains(self, d: Date) -> bool {
        self.has(Self::index_of(d))
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// "Every day" or the days in week order, e.g. "Mon, Thu".
    pub fn label(self, lang: Lang) -> String {
        if self == Self::ALL {
            return lang.t("Every day").into();
        }
        let days: Vec<&str> = (0..7)
            .filter(|i| self.has(*i))
            .map(|i| weekday_short(i, lang))
            .collect();
        lang.f1("Every week: {0}", days.join(", "))
    }
}

#[derive(Clone, Debug, sqlx::FromRow, serde::Serialize, serde::Deserialize)]
pub struct Activity {
    pub id: i64,
    /// The English name of a listed sport, or the name of a custom one.
    pub sport: String,
    /// The listed sport ([`sports::SPORTS`]); `None` for a custom sport.
    #[serde(default)]
    pub sport_key: Option<String>,
    /// [`Intensity`] of a custom sport; `None` for a listed sport and for
    /// custom sports recorded before intensities existed.
    #[serde(default)]
    pub intensity: Option<String>,
    pub duration_min: i64,
    pub note: Option<String>,
    pub start_date: String,
    /// [`Weekdays`] bits; 0 for a one-off activity.
    pub weekdays: i64,
    pub end_date: Option<String>,
}

impl Activity {
    pub fn repeat(&self) -> Weekdays {
        Weekdays::from_bits(self.weekdays).unwrap_or_default()
    }

    pub fn repeats(&self) -> bool {
        !self.repeat().is_empty()
    }

    pub fn listed_sport(&self) -> Option<&'static Sport> {
        self.sport_key.as_deref().and_then(sports::find)
    }

    pub fn intensity(&self) -> Option<Intensity> {
        self.intensity.as_deref().and_then(Intensity::parse)
    }

    /// The sport's name in `lang`; a custom sport keeps the name typed.
    pub fn name(&self, lang: Lang) -> &str {
        match self.listed_sport() {
            Some(s) => s.name(lang),
            None => &self.sport,
        }
    }

    /// METs of the sport, when known.
    pub fn met(&self) -> Option<f64> {
        match self.listed_sport() {
            Some(s) => Some(s.met),
            None => self.intensity().map(Intensity::met),
        }
    }

    /// Estimated energy above rest for a body of `weight_kg`.
    pub fn kcal(&self, weight_kg: f64) -> Option<f64> {
        self.met()
            .map(|met| sports::net_kcal(met, self.duration_min, weight_kg))
    }

    /// Whether the activity is planned for `d`, ignoring skipped days.
    pub fn occurs_on(&self, d: Date) -> bool {
        let Some(start) = parse_date(&self.start_date) else {
            return false;
        };
        let repeat = self.repeat();
        if repeat.is_empty() {
            return d == start;
        }
        let before_end = self
            .end_date
            .as_deref()
            .and_then(parse_date)
            .is_none_or(|end| d <= end);
        d >= start && before_end && repeat.contains(d)
    }
}

/// A day on which a repeating activity did not take place.
#[derive(Clone, Debug, sqlx::FromRow, serde::Serialize, serde::Deserialize)]
pub struct ActivitySkip {
    pub activity_id: i64,
    pub skip_date: String,
}

#[derive(Clone, Debug, sqlx::FromRow, serde::Serialize, serde::Deserialize)]
pub struct DailySteps {
    pub step_date: String,
    pub steps: i64,
}

/// What sport an activity is.
#[derive(Clone, Debug, PartialEq)]
pub enum SportKind {
    Listed(&'static Sport),
    Custom { name: String, intensity: Intensity },
}

impl SportKind {
    /// Stored in `activities.sport`: a listed sport's English name.
    fn name(&self) -> &str {
        match self {
            SportKind::Listed(s) => s.name(Lang::En),
            SportKind::Custom { name, .. } => name,
        }
    }

    fn key(&self) -> Option<&'static str> {
        match self {
            SportKind::Listed(s) => Some(s.key),
            SportKind::Custom { .. } => None,
        }
    }

    fn intensity(&self) -> Option<&'static str> {
        match self {
            SportKind::Listed(_) => None,
            SportKind::Custom { intensity, .. } => Some(intensity.as_str()),
        }
    }
}

/// The values of an activity form, already validated.
#[derive(Clone, Debug, PartialEq)]
pub struct NewActivity {
    pub kind: SportKind,
    pub duration_min: i64,
    pub note: Option<String>,
    pub start: Date,
    pub repeat: Weekdays,
    pub end: Option<Date>,
}

/// Reads a whole number such as "8000", "8 000", "8,000" or "8.000".
/// Separators are only accepted between groups of three digits, so "8,5"
/// is refused rather than read as 85.
pub fn parse_count(input: &str) -> Option<i64> {
    let t: String = input
        .trim()
        .chars()
        .filter(|c| !matches!(c, ' ' | '\u{a0}' | '\u{202f}' | '\''))
        .collect();
    if t.is_empty() || t.len() > 12 {
        return None;
    }
    let groups: Vec<&str> = t.split([',', '.']).collect();
    let grouped_ok = groups.len() == 1
        || (!groups[0].is_empty()
            && groups[0].len() <= 3
            && groups[1..].iter().all(|g| g.len() == 3));
    if !grouped_ok || !groups.iter().all(|g| g.bytes().all(|b| b.is_ascii_digit())) {
        return None;
    }
    groups.concat().parse().ok()
}

pub fn check_steps(input: &str, lang: Lang) -> Result<i64, String> {
    if input.trim().is_empty() {
        return Err(lang.t("Enter a number of steps.").into());
    }
    match parse_count(input) {
        Some(n) if n <= MAX_STEPS => Ok(n),
        Some(_) => Err(lang.f1("Enter at most {0} steps.", MAX_STEPS)),
        None => Err(lang
            .t("Enter the steps as a whole number, for example 8000.")
            .into()),
    }
}

pub fn check_duration(input: &str, lang: Lang) -> Result<i64, String> {
    if input.trim().is_empty() {
        return Err(lang.t("Enter how many minutes it lasted.").into());
    }
    match parse_count(input) {
        Some(n) if (1..=MAX_DURATION_MIN).contains(&n) => Ok(n),
        Some(_) => Err(lang.f1(
            "Enter a duration between 1 and {0} minutes.",
            MAX_DURATION_MIN,
        )),
        None => Err(lang
            .t("Enter the duration in whole minutes, for example 45.")
            .into()),
    }
}

/// Activities taking place on `d`, in the order they were added.
pub async fn for_date(db: &SqlitePool, d: Date) -> AppResult<Vec<Activity>> {
    let day = d.to_string();
    // The weekday is checked in Rust; the query only bounds the rows to
    // this day's one-off activities and the series running on it.
    let rows = sqlx::query_as::<_, Activity>(
        "SELECT id, sport, sport_key, intensity, duration_min, note, start_date, weekdays, end_date FROM activities a
         WHERE start_date <= ?1
           AND ((weekdays = 0 AND start_date = ?1)
                OR (weekdays <> 0 AND (end_date IS NULL OR end_date >= ?1)))
           AND NOT EXISTS (SELECT 1 FROM activity_skips s
                           WHERE s.activity_id = a.id AND s.skip_date = ?1)
         ORDER BY created_at, id",
    )
    .bind(day)
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().filter(|a| a.occurs_on(d)).collect())
}

pub async fn get(db: &SqlitePool, id: i64) -> AppResult<Option<Activity>> {
    Ok(sqlx::query_as::<_, Activity>(
        "SELECT id, sport, sport_key, intensity, duration_min, note, start_date, weekdays, end_date
             FROM activities WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(db)
    .await?)
}

fn end_text(a: &NewActivity) -> Option<String> {
    // An end only means something for a repeating activity.
    a.end
        .filter(|_| !a.repeat.is_empty())
        .map(|d| d.to_string())
}

pub async fn create(db: &SqlitePool, a: &NewActivity) -> AppResult<i64> {
    let id = sqlx::query_scalar(
        "INSERT INTO activities
             (sport, sport_key, intensity, duration_min, note, start_date, weekdays, end_date)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(a.kind.name())
    .bind(a.kind.key())
    .bind(a.kind.intensity())
    .bind(a.duration_min)
    .bind(&a.note)
    .bind(a.start.to_string())
    .bind(a.repeat.bits())
    .bind(end_text(a))
    .fetch_one(db)
    .await?;
    Ok(id)
}

/// Changes the activity on every day it takes place. Skipped days stay
/// skipped.
pub async fn update(db: &SqlitePool, id: i64, a: &NewActivity) -> AppResult<bool> {
    let res = sqlx::query(
        "UPDATE activities SET sport = ?, sport_key = ?, intensity = ?, duration_min = ?,
             note = ?, start_date = ?, weekdays = ?, end_date = ?,
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
         WHERE id = ?",
    )
    .bind(a.kind.name())
    .bind(a.kind.key())
    .bind(a.kind.intensity())
    .bind(a.duration_min)
    .bind(&a.note)
    .bind(a.start.to_string())
    .bind(a.repeat.bits())
    .bind(end_text(a))
    .bind(id)
    .execute(db)
    .await?;
    Ok(res.rows_affected() > 0)
}

/// Removes one day of a repeating activity, keeping the others.
pub async fn skip(db: &SqlitePool, id: i64, d: Date) -> AppResult<()> {
    sqlx::query("INSERT OR IGNORE INTO activity_skips (activity_id, skip_date) VALUES (?, ?)")
        .bind(id)
        .bind(d.to_string())
        .execute(db)
        .await?;
    Ok(())
}

/// Deletes the activity on every day it takes place.
pub async fn delete(db: &SqlitePool, id: i64) -> AppResult<bool> {
    let res = sqlx::query("DELETE FROM activities WHERE id = ?")
        .bind(id)
        .execute(db)
        .await?;
    Ok(res.rows_affected() > 0)
}

/// Steps entered for `d`, if any.
pub async fn steps_on(db: &SqlitePool, d: Date) -> AppResult<Option<i64>> {
    Ok(
        sqlx::query_scalar("SELECT steps FROM daily_steps WHERE step_date = ?")
            .bind(d.to_string())
            .fetch_optional(db)
            .await?,
    )
}

pub async fn set_steps(db: &SqlitePool, d: Date, steps: i64) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO daily_steps (step_date, steps) VALUES (?, ?)
         ON CONFLICT (step_date) DO UPDATE SET steps = excluded.steps,
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')",
    )
    .bind(d.to_string())
    .bind(steps)
    .execute(db)
    .await?;
    Ok(())
}

/// Forgets the steps entered for `d`.
pub async fn clear_steps(db: &SqlitePool, d: Date) -> AppResult<()> {
    sqlx::query("DELETE FROM daily_steps WHERE step_date = ?")
        .bind(d.to_string())
        .execute(db)
        .await?;
    Ok(())
}

pub fn check_step_goal(input: &str, lang: Lang) -> Result<i64, String> {
    match parse_count(input) {
        Some(n) if (MIN_STEP_GOAL..=MAX_STEPS).contains(&n) => Ok(n),
        _ => Err(lang.f2(
            "Enter a step goal between {0} and {1}.",
            MIN_STEP_GOAL,
            MAX_STEPS,
        )),
    }
}

pub async fn step_goal(db: &SqlitePool) -> AppResult<i64> {
    Ok(
        sqlx::query_scalar("SELECT step_goal FROM profile WHERE id = 1")
            .fetch_one(db)
            .await?,
    )
}

pub async fn set_step_goal(db: &SqlitePool, goal: i64) -> AppResult<()> {
    sqlx::query(
        "UPDATE profile SET step_goal = ?,
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = 1",
    )
    .bind(goal)
    .execute(db)
    .await?;
    Ok(())
}

/// Approximate energy spent walking `steps`, above what the body uses at
/// rest: distance from a stride of [`STRIDE_PER_HEIGHT`] × height, times
/// [`NET_KCAL_PER_KG_KM`] × body weight. Pace, slopes and individual
/// differences are ignored, so this is an estimate, not a measurement.
pub fn walking_kcal(steps: i64, height_cm: f64, weight_kg: f64) -> f64 {
    let km = steps as f64 * height_cm * STRIDE_PER_HEIGHT / 100_000.0;
    NET_KCAL_PER_KG_KM * weight_kg * km
}

/// Height and weight used for the walking estimate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Body {
    pub height_cm: f64,
    pub weight_kg: f64,
}

/// The weight on `d`: the last weigh-in on or before it, so later
/// weigh-ins never change past days, otherwise the weight in the goal
/// settings or the first weigh-in.
pub async fn weight_on(db: &SqlitePool, d: Date) -> AppResult<Option<f64>> {
    let on_or_before: Option<f64> = sqlx::query_scalar(
        "SELECT weight_kg FROM weight_entries WHERE entry_date <= ?
         ORDER BY entry_date DESC LIMIT 1",
    )
    .bind(d.to_string())
    .fetch_optional(db)
    .await?;
    if let Some(w) = on_or_before {
        return Ok(Some(w));
    }
    if let Some(w) = crate::goals::load(db).await?.weight_kg {
        return Ok(Some(w));
    }
    Ok(
        sqlx::query_scalar("SELECT weight_kg FROM weight_entries ORDER BY entry_date LIMIT 1")
            .fetch_optional(db)
            .await?,
    )
}

/// The height from the goal settings and the weight on `d` ([`weight_on`]).
pub async fn body_on(db: &SqlitePool, d: Date) -> AppResult<Option<Body>> {
    let Some(height_cm) = crate::goals::load(db).await?.height_cm else {
        return Ok(None);
    };
    Ok(weight_on(db, d).await?.map(|weight_kg| Body {
        height_cm,
        weight_kg,
    }))
}

/// Steps entered for a day and the energy they are estimated to represent.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StepEnergy {
    pub steps: i64,
    pub kcal: f64,
}

/// `None` when no steps were entered for `d` or height or weight is unknown.
pub async fn step_energy_on(db: &SqlitePool, d: Date) -> AppResult<Option<StepEnergy>> {
    let Some(steps) = steps_on(db, d).await? else {
        return Ok(None);
    };
    Ok(body_on(db, d).await?.map(|b| StepEnergy {
        steps,
        kcal: walking_kcal(steps, b.height_cm, b.weight_kg),
    }))
}

/// The day's sport sessions with an estimate and the energy they are
/// estimated to use together.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SportEnergy {
    pub minutes: i64,
    pub kcal: f64,
}

/// `None` when no session on `d` has an estimate or the weight is unknown.
pub async fn sport_energy_on(db: &SqlitePool, d: Date) -> AppResult<Option<SportEnergy>> {
    let list = for_date(db, d).await?;
    if list.iter().all(|a| a.met().is_none()) {
        return Ok(None);
    }
    Ok(weight_on(db, d).await?.map(|w| sport_energy(&list, w)))
}

pub fn sport_energy(list: &[Activity], weight_kg: f64) -> SportEnergy {
    list.iter()
        .filter_map(|a| a.kcal(weight_kg).map(|k| (a.duration_min, k)))
        .fold(
            SportEnergy {
                minutes: 0,
                kcal: 0.0,
            },
            |e, (m, k)| SportEnergy {
                minutes: e.minutes + m,
                kcal: e.kcal + k,
            },
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Lang::{En, Fr};
    use jiff::civil::date;

    fn activity(start: &str, weekdays: i64, end: Option<&str>) -> Activity {
        Activity {
            id: 1,
            sport: "Swimming".into(),
            sport_key: Some("swimming".into()),
            intensity: None,
            duration_min: 45,
            note: None,
            start_date: start.into(),
            weekdays,
            end_date: end.map(Into::into),
        }
    }

    #[test]
    fn counts_accept_thousands_separators_only_between_groups_of_three() {
        for (input, n) in [
            ("8000", 8000),
            (" 8 000 ", 8000),
            ("8,000", 8000),
            ("8.000", 8000),
            ("8\u{202f}000", 8000),
            ("12'345", 12345),
            ("0", 0),
            ("1,234,567", 1_234_567),
        ] {
            assert_eq!(parse_count(input), Some(n), "{input}");
        }
        for bad in [
            "", "8,5", "8.00", ",000", "-5", "1e4", "abc", "45min", "1.5",
        ] {
            assert_eq!(parse_count(bad), None, "{bad}");
        }
    }

    #[test]
    fn steps_and_duration_bounds() {
        assert_eq!(check_steps("10000", En), Ok(10_000));
        assert_eq!(check_steps("0", En), Ok(0));
        assert!(check_steps("", En).is_err());
        assert!(check_steps("100001", En).is_err());
        assert!(check_steps("-1", En).is_err());
        assert_eq!(check_duration("45", En), Ok(45));
        assert!(check_duration("0", En).is_err());
        assert!(check_duration("1441", En).is_err());
        assert!(check_duration("1,5", En).is_err());
    }

    #[test]
    fn one_off_activity_happens_on_its_date_only() {
        let a = activity("2026-10-21", 0, None);
        assert!(a.occurs_on(date(2026, 10, 21)));
        assert!(!a.occurs_on(date(2026, 10, 28)));
        assert!(!a.repeats());
    }

    #[test]
    fn weekly_activity_follows_weekdays_start_and_end() {
        // Mondays and Thursdays, from Wednesday 21 Oct 2026 to Thursday 5 Nov.
        let days = Weekdays::NONE.with(0).with(3);
        let a = activity("2026-10-21", days.bits(), Some("2026-11-05"));
        assert!(!a.occurs_on(date(2026, 10, 19)), "Monday before the start");
        assert!(
            !a.occurs_on(date(2026, 10, 21)),
            "start date is a Wednesday"
        );
        assert!(a.occurs_on(date(2026, 10, 22)), "first Thursday");
        assert!(a.occurs_on(date(2026, 10, 26)), "Monday");
        assert!(!a.occurs_on(date(2026, 10, 27)), "Tuesday");
        assert!(a.occurs_on(date(2026, 11, 5)), "end date is included");
        assert!(!a.occurs_on(date(2026, 11, 9)), "Monday after the end");
        let open = activity("2026-10-21", days.bits(), None);
        assert!(open.occurs_on(date(2030, 1, 3)), "no end");
    }

    #[test]
    fn weekday_sets() {
        assert_eq!(Weekdays::of(date(2026, 10, 25)), Weekdays::NONE.with(6));
        assert_eq!(Weekdays::from_bits(128), None);
        assert_eq!(Weekdays::from_bits(-1), None);
        assert_eq!(Weekdays::NONE.with(9), Weekdays::NONE);
        assert_eq!(Weekdays::ALL.label(En), "Every day");
        assert_eq!(
            Weekdays::NONE.with(3).with(0).label(En),
            "Every week: Mon, Thu"
        );
        assert_eq!(Weekdays::NONE.with(5).label(Fr), "Chaque semaine : sam.");
    }

    #[tokio::test]
    async fn day_lists_one_offs_series_and_leaves_out_skipped_days() {
        let db = crate::db::connect_memory().await.unwrap();
        let tuesday = date(2026, 10, 20);
        let thursday = date(2026, 10, 22);
        let weekly = create(
            &db,
            &NewActivity {
                kind: SportKind::Listed(sports::find("running").unwrap()),
                duration_min: 30,
                note: None,
                start: tuesday,
                repeat: Weekdays::of(tuesday).with(Weekdays::index_of(thursday)),
                end: None,
            },
        )
        .await
        .unwrap();
        create(
            &db,
            &NewActivity {
                kind: SportKind::Custom {
                    name: "Bouldering".into(),
                    intensity: Intensity::Vigorous,
                },
                duration_min: 90,
                note: Some("Indoor".into()),
                start: thursday,
                repeat: Weekdays::NONE,
                end: Some(date(2026, 12, 1)),
            },
        )
        .await
        .unwrap();

        let names = |v: Vec<Activity>| v.into_iter().map(|a| a.sport).collect::<Vec<_>>();
        assert_eq!(
            names(for_date(&db, thursday).await.unwrap()),
            ["Running", "Bouldering"]
        );
        assert_eq!(
            names(for_date(&db, date(2026, 10, 27)).await.unwrap()),
            ["Running"]
        );
        assert!(for_date(&db, date(2026, 10, 21)).await.unwrap().is_empty());

        let climbing = &for_date(&db, thursday).await.unwrap()[1];
        assert_eq!(climbing.end_date, None, "a one-off activity has no end");

        skip(&db, weekly, thursday).await.unwrap();
        skip(&db, weekly, thursday).await.unwrap();
        assert_eq!(
            names(for_date(&db, thursday).await.unwrap()),
            ["Bouldering"]
        );
        assert_eq!(
            names(for_date(&db, date(2026, 10, 29)).await.unwrap()),
            ["Running"]
        );

        assert!(delete(&db, weekly).await.unwrap());
        let skips: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM activity_skips")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(skips, 0, "skipped days go with the activity");
    }

    #[test]
    fn walking_energy_scales_with_steps_height_and_weight() {
        // 10,000 steps at 175 cm is about 7.2 km; at 75 kg about 272 kcal.
        let kcal = walking_kcal(10_000, 175.0, 75.0);
        assert!((kcal - 271.69).abs() < 0.01, "{kcal}");
        assert_eq!(walking_kcal(0, 175.0, 75.0), 0.0);
        assert!((walking_kcal(5_000, 175.0, 75.0) * 2.0 - kcal).abs() < 1e-9);
        assert!(walking_kcal(10_000, 175.0, 90.0) > kcal);
        assert!(walking_kcal(10_000, 190.0, 75.0) > kcal);
    }

    #[test]
    fn step_goal_bounds() {
        assert_eq!(check_step_goal("10 000", En), Ok(10_000));
        assert_eq!(check_step_goal("1000", En), Ok(1_000));
        assert!(check_step_goal("999", En).is_err());
        assert!(check_step_goal("100001", En).is_err());
        assert!(check_step_goal("", En).is_err());
    }

    #[tokio::test]
    async fn steps_and_their_energy_per_day() {
        let db = crate::db::connect_memory().await.unwrap();
        let d = date(2026, 10, 24);
        assert_eq!(step_goal(&db).await.unwrap(), DEFAULT_STEP_GOAL);
        set_step_goal(&db, 8000).await.unwrap();
        assert_eq!(step_goal(&db).await.unwrap(), 8000);

        assert_eq!(steps_on(&db, d).await.unwrap(), None);
        set_steps(&db, d, 7200).await.unwrap();
        set_steps(&db, d, 10_000).await.unwrap();
        assert_eq!(steps_on(&db, d).await.unwrap(), Some(10_000));
        assert_eq!(
            step_energy_on(&db, d).await.unwrap(),
            None,
            "no estimate without height and weight"
        );

        let g = crate::goals::GoalSettings {
            height_cm: Some(175.0),
            weight_kg: Some(80.0),
            goal: "maintain".into(),
            ..Default::default()
        };
        crate::goals::save(&db, &g).await.unwrap();
        let goal_weight = step_energy_on(&db, d).await.unwrap().unwrap();
        assert_eq!(goal_weight.kcal, walking_kcal(10_000, 175.0, 80.0));

        // The weigh-in on or before the day counts; a later one does not.
        crate::weight::record(&db, date(2026, 10, 20), 75.0, None)
            .await
            .unwrap();
        crate::weight::record(&db, date(2026, 10, 30), 70.0, None)
            .await
            .unwrap();
        let e = step_energy_on(&db, d).await.unwrap().unwrap();
        assert_eq!(e.kcal, walking_kcal(10_000, 175.0, 75.0));

        clear_steps(&db, d).await.unwrap();
        assert_eq!(step_energy_on(&db, d).await.unwrap(), None);
    }

    #[tokio::test]
    async fn sport_energy_uses_the_weight_of_the_day() {
        let db = crate::db::connect_memory().await.unwrap();
        let d = date(2026, 10, 24);
        let add = |kind: SportKind, minutes| NewActivity {
            kind,
            duration_min: minutes,
            note: None,
            start: d,
            repeat: Weekdays::NONE,
            end: None,
        };
        create(
            &db,
            &add(SportKind::Listed(sports::find("running").unwrap()), 30),
        )
        .await
        .unwrap();
        assert_eq!(sport_energy_on(&db, d).await.unwrap(), None, "no weight");

        crate::weight::record(&db, date(2026, 10, 1), 70.0, None)
            .await
            .unwrap();
        let e = sport_energy_on(&db, d).await.unwrap().unwrap();
        assert_eq!(e.minutes, 30);
        assert!((e.kcal - 280.0).abs() < 1e-9, "{}", e.kcal);

        let custom = SportKind::Custom {
            name: "Parkour".into(),
            intensity: Intensity::Moderate,
        };
        create(&db, &add(custom, 60)).await.unwrap();
        // A session recorded before sports had an estimate counts for nothing.
        sqlx::query(
            "INSERT INTO activities (sport, duration_min, start_date) VALUES ('Old', 45, ?)",
        )
        .bind(d.to_string())
        .execute(&db)
        .await
        .unwrap();
        let e = sport_energy_on(&db, d).await.unwrap().unwrap();
        assert_eq!(e.minutes, 90);
        assert!((e.kcal - 280.0 - 3.5 * 70.0).abs() < 1e-9, "{}", e.kcal);
        let list = for_date(&db, d).await.unwrap();
        assert_eq!(list[0].name(Lang::Fr), "Course à pied");
        assert_eq!(list[1].name(Lang::Fr), "Parkour");
        assert_eq!(list[2].kcal(70.0), None);
    }
}
