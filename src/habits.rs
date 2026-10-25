//! Usual activity, asked at first-run setup: roughly how many steps a day,
//! on which days and how long at the gym, and, for someone who plays a
//! sport, which one, on which days and how long. The gym is its own
//! question because Korpus plans it with programs (see [`crate::gym`]);
//! setup can start one from a template.
//!
//! The answers suggest the activity level used by the calorie estimate in
//! Goals (see [`suggested_activity`]) and the sport offered first when
//! adding an activity. They never change a level the user has already set.
//! The gym and the sport also become activities repeating on their days
//! (see [`plan`]), so their energy counts in those days' totals.

use std::collections::BTreeMap;

use jiff::civil::Date;
use sqlx::SqlitePool;

use crate::activity::{self, NewActivity, SportKind, Weekdays};
use crate::error::AppResult;
use crate::forms::FieldErrors;
use crate::goals::Activity;
use crate::gym::starter::{self, Starter};
use crate::i18n::Lang;
use crate::profile::Profile;
use crate::sports::{self, Sport};

/// Answer to "How many steps do you take on a usual day?".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UsualSteps {
    Under5k,
    From5kTo10k,
    From10kTo15k,
    Over15k,
}

impl UsualSteps {
    pub const ALL: [UsualSteps; 4] = [
        UsualSteps::Under5k,
        UsualSteps::From5kTo10k,
        UsualSteps::From10kTo15k,
        UsualSteps::Over15k,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            UsualSteps::Under5k => "under_5k",
            UsualSteps::From5kTo10k => "5k_10k",
            UsualSteps::From10kTo15k => "10k_15k",
            UsualSteps::Over15k => "over_15k",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.as_str() == s)
    }

    pub fn label(self, lang: Lang) -> &'static str {
        match self {
            UsualSteps::Under5k => lang.t("Under 5,000 steps"),
            UsualSteps::From5kTo10k => lang.t("5,000 to 10,000 steps"),
            UsualSteps::From10kTo15k => lang.t("10,000 to 15,000 steps"),
            UsualSteps::Over15k => lang.t("Over 15,000 steps"),
        }
    }

    pub fn detail(self, lang: Lang) -> &'static str {
        match self {
            UsualSteps::Under5k => lang.t("Mostly sitting, e.g. office work and travel by car"),
            UsualSteps::From5kTo10k => lang.t("Some walking during the day"),
            UsualSteps::From10kTo15k => lang.t("On my feet a lot, or long daily walks"),
            UsualSteps::Over15k => lang.t("A physical job, or walking most of the day"),
        }
    }

    /// Walking above everyday movement in a usual week, in MET-hours: the
    /// steps beyond 5,000 a day (which Sedentary already includes) at the
    /// middle of the range, walked at 100 steps a minute and 3.5 METs (the
    /// Compendium's walking at 3 mph), counting only what is above rest.
    pub fn net_met_hours_per_week(self) -> f64 {
        let steps_per_day: f64 = match self {
            UsualSteps::Under5k => 0.0,
            UsualSteps::From5kTo10k => 7_500.0,
            UsualSteps::From10kTo15k => 12_500.0,
            UsualSteps::Over15k => 17_500.0,
        };
        let extra_hours = (steps_per_day - 5_000.0).max(0.0) / WALKING_STEPS_PER_MIN / 60.0 * 7.0;
        extra_hours * (WALKING_MET - 1.0)
    }
}

const WALKING_STEPS_PER_MIN: f64 = 100.0;
const WALKING_MET: f64 = 3.5;

/// What gym training counts as. Not offered among the sports, since the
/// gym has its own questions.
pub const GYM_SPORT: &str = "traditional_strength_training";

/// Sports offered as cards; every other listed sport is under "Other".
pub const POPULAR_SPORTS: [&str; 7] = [
    "running", "cycling", "hiking", "swimming", "soccer", "yoga", "tennis",
];
/// Typical session lengths offered, in minutes.
pub const MINUTE_CHOICES: [i64; 5] = [30, 45, 60, 90, 120];

/// The sport someone plays, or the gym, and how much.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SportHabit {
    pub sport: &'static Sport,
    pub sessions_per_week: i64,
    pub minutes: i64,
    /// The days of the week; empty when only the number of sessions is
    /// known, as in the profile, which does not keep the days.
    pub days: Weekdays,
}

impl SportHabit {
    /// Energy above rest in a usual week, in MET-hours (about kcal per kg
    /// of body weight), as for recorded sport.
    pub fn net_met_hours_per_week(&self) -> f64 {
        (self.sport.met - 1.0).max(0.0) * (self.sessions_per_week * self.minutes) as f64 / 60.0
    }
}

/// The answers given at setup. Each question may be skipped.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Habits {
    pub steps: Option<UsualSteps>,
    /// `None` when the question was skipped.
    pub plays_sport: Option<bool>,
    /// Set exactly when `plays_sport` is `Some(true)`.
    pub sport: Option<SportHabit>,
    /// `None` when the question was skipped.
    pub goes_to_gym: Option<bool>,
    /// Set exactly when `goes_to_gym` is `Some(true)`; its sport is
    /// [`GYM_SPORT`].
    pub gym: Option<SportHabit>,
}

/// The habit when `answer` is yes and its details are all there, and the
/// answer as it stands with them: a yes without details is no answer.
fn habit(
    answer: Option<bool>,
    sport: Option<&'static Sport>,
    sessions: Option<i64>,
    minutes: Option<i64>,
) -> (Option<bool>, Option<SportHabit>) {
    match (answer, sport, sessions, minutes) {
        (Some(true), Some(sport), Some(sessions_per_week), Some(minutes)) => (
            Some(true),
            Some(SportHabit {
                sport,
                sessions_per_week,
                minutes,
                days: Weekdays::NONE,
            }),
        ),
        _ => (answer.filter(|a| !a), None),
    }
}

fn in_range(h: Option<SportHabit>) -> bool {
    h.is_none_or(|s| (1..=7).contains(&s.sessions_per_week) && MINUTE_CHOICES.contains(&s.minutes))
}

impl Habits {
    pub fn from_profile(p: &Profile) -> Self {
        let (plays_sport, sport) = habit(
            p.plays_sport,
            p.sport_key.as_deref().and_then(sports::find),
            p.sport_sessions,
            p.sport_minutes,
        );
        let (goes_to_gym, gym) = habit(
            p.goes_to_gym,
            sports::find(GYM_SPORT),
            p.gym_sessions,
            p.gym_minutes,
        );
        Habits {
            steps: p.usual_steps.as_deref().and_then(UsualSteps::parse),
            plays_sport,
            sport,
            goes_to_gym,
            gym,
        }
    }

    /// Whether the profile's answer columns are exactly what these answers
    /// store, e.g. for an import: no unknown values, and sport details only
    /// with a sport.
    pub fn matches_profile(&self, p: &Profile) -> bool {
        in_range(self.sport)
            && in_range(self.gym)
            && p.usual_steps.as_deref() == self.steps.map(UsualSteps::as_str)
            && p.plays_sport == self.plays_sport
            && p.sport_key.as_deref() == self.sport.map(|s| s.sport.key)
            && p.sport_sessions == self.sport.map(|s| s.sessions_per_week)
            && p.sport_minutes == self.sport.map(|s| s.minutes)
            && p.goes_to_gym == self.goes_to_gym
            && p.gym_sessions == self.gym.map(|s| s.sessions_per_week)
            && p.gym_minutes == self.gym.map(|s| s.minutes)
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_none() && self.plays_sport.is_none() && self.goes_to_gym.is_none()
    }
}

/// MET-hours a week of activity above Sedentary that count as one more
/// activity level. Each level adds 17.5% of resting energy, which for an
/// average adult (about 22 kcal per kg a day) is some 27 MET-hours a week;
/// the smaller step allows for what the multipliers include besides
/// exercise, and still keeps the suggestion well below the levels'
/// descriptions, which are known to overestimate.
pub const MET_HOURS_PER_LEVEL: f64 = 20.0;

/// The activity level these answers suggest, from the energy of the usual
/// walking, gym and sport together, rounded to the nearest level. `None` when
/// no question was answered.
pub fn suggested_activity(h: &Habits) -> Option<Activity> {
    if h.is_empty() {
        return None;
    }
    let met_hours = h.steps.map_or(0.0, UsualSteps::net_met_hours_per_week)
        + [h.sport, h.gym]
            .iter()
            .flatten()
            .map(SportHabit::net_met_hours_per_week)
            .sum::<f64>();
    let level = (met_hours / MET_HOURS_PER_LEVEL).round() as usize;
    Some(Activity::ALL[level.min(Activity::ALL.len() - 1)])
}

/// The setup questions as submitted.
#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(default)]
pub struct HabitsForm {
    /// A [`UsualSteps`] value, "unsure" or empty.
    pub usual_steps: String,
    /// "yes", "no" or empty.
    pub plays_sport: String,
    /// A popular sport's key, or "other" to use `other_sport`.
    pub sport_key: String,
    pub other_sport: String,
    pub sport_minutes: String,
    /// "yes", "no" or empty.
    pub gym: String,
    pub gym_minutes: String,
    /// A starter program's key, or empty or [`NO_PROGRAM`] for none.
    pub gym_program: String,
    /// The weekday boxes, `gym_day0` … `sport_day6` (0 is Monday), see
    /// [`HabitsForm::has_day`]. The form's other fields land here too and
    /// are ignored.
    #[serde(flatten)]
    pub days: BTreeMap<String, String>,
}

/// `sport_key` value of the "Other" card.
pub const OTHER: &str = "other";
/// `gym_program` value of the "Not now" card.
pub const NO_PROGRAM: &str = "none";

/// A yes/no answer; `None` when skipped.
fn yes_no(value: &str, field: &'static str, lang: Lang, errors: &mut FieldErrors) -> Option<bool> {
    match value {
        "" => None,
        "yes" => Some(true),
        "no" => Some(false),
        _ => {
            errors.add(field, lang.t("Choose one of the options."));
            None
        }
    }
}

fn yes_no_value(answer: Option<bool>) -> String {
    match answer {
        Some(true) => "yes".into(),
        Some(false) => "no".into(),
        None => String::new(),
    }
}

/// At least one day, and minutes a session from the offered choices.
fn frequency(
    days: Weekdays,
    minutes: &str,
    fields: (&'static str, &'static str),
    lang: Lang,
    errors: &mut FieldErrors,
) -> Option<(Weekdays, i64)> {
    if days.is_empty() {
        errors.add(fields.0, lang.t("Choose at least one day of the week."));
    }
    let minutes = minutes
        .parse::<i64>()
        .ok()
        .filter(|n| MINUTE_CHOICES.contains(n));
    if minutes.is_none() {
        errors.add(fields.1, lang.t("Choose how long a session usually lasts."));
    }
    Some((days, minutes?)).filter(|(d, _)| !d.is_empty())
}

fn habit_on(sport: &'static Sport, days: Weekdays, minutes: i64) -> SportHabit {
    SportHabit {
        sport,
        sessions_per_week: days.count() as i64,
        minutes,
        days,
    }
}

impl HabitsForm {
    pub fn parse(&self, lang: Lang, errors: &mut FieldErrors) -> Habits {
        let steps = match self.usual_steps.as_str() {
            "" | "unsure" => None,
            s => UsualSteps::parse(s).or_else(|| {
                errors.add("usual_steps", lang.t("Choose one of the options."));
                None
            }),
        };
        let plays_sport = yes_no(&self.plays_sport, "plays_sport", lang, errors);
        let sport = if plays_sport == Some(true) {
            self.sport_habit(lang, errors)
        } else {
            None
        };
        let goes_to_gym = yes_no(&self.gym, "gym", lang, errors);
        let gym = if goes_to_gym == Some(true) {
            let fields = ("gym_days", "gym_minutes");
            let days = self.days_of("gym");
            frequency(days, &self.gym_minutes, fields, lang, errors)
                .and_then(|(days, minutes)| Some(habit_on(sports::find(GYM_SPORT)?, days, minutes)))
        } else {
            None
        };
        Habits {
            steps,
            plays_sport,
            sport,
            goes_to_gym,
            gym,
        }
    }

    /// The starter program chosen to begin with, if any. Only asked of
    /// someone who goes to the gym.
    pub fn starter(&self, lang: Lang, errors: &mut FieldErrors) -> Option<&'static Starter> {
        match self.gym_program.as_str() {
            "" | NO_PROGRAM => None,
            key => starter::find(key).or_else(|| {
                errors.add("gym_program", lang.t("Choose one of the options."));
                None
            }),
        }
    }

    fn sport_habit(&self, lang: Lang, errors: &mut FieldErrors) -> Option<SportHabit> {
        let key = match self.sport_key.as_str() {
            OTHER => self.other_sport.as_str(),
            k => k,
        };
        // The gym has its own questions.
        let sport = sports::find(key).filter(|s| s.key != GYM_SPORT);
        if sport.is_none() {
            errors.add("sport_key", lang.t("Choose a sport."));
        }
        let fields = ("sport_days", "sport_minutes");
        let days = self.days_of("sport");
        let (days, minutes) = frequency(days, &self.sport_minutes, fields, lang, errors)?;
        Some(habit_on(sport?, days, minutes))
    }

    /// Whether the box for weekday `index` (0 is Monday) of `prefix`
    /// ("gym" or "sport") is ticked.
    pub fn has_day(&self, prefix: &str, index: &str) -> bool {
        self.days
            .get(&format!("{prefix}_day{index}"))
            .is_some_and(|v| !v.is_empty())
    }

    fn days_of(&self, prefix: &str) -> Weekdays {
        (0..7)
            .filter(|i| self.has_day(prefix, &i.to_string()))
            .fold(Weekdays::NONE, Weekdays::with)
    }

    pub fn from_habits(h: &Habits) -> Self {
        let sport_key = h.sport.map(|s| s.sport.key).unwrap_or_default();
        let popular = POPULAR_SPORTS.contains(&sport_key);
        HabitsForm {
            usual_steps: h.steps.map(UsualSteps::as_str).unwrap_or_default().into(),
            plays_sport: yes_no_value(h.plays_sport),
            sport_key: if sport_key.is_empty() || popular {
                sport_key.into()
            } else {
                OTHER.into()
            },
            other_sport: if popular { "" } else { sport_key }.into(),
            sport_minutes: h.sport.map(|s| s.minutes.to_string()).unwrap_or_default(),
            gym: yes_no_value(h.goes_to_gym),
            gym_minutes: h.gym.map(|s| s.minutes.to_string()).unwrap_or_default(),
            gym_program: String::new(),
            days: [("gym", h.gym), ("sport", h.sport)]
                .into_iter()
                .flat_map(|(prefix, habit)| {
                    let days = habit.map(|s| s.days).unwrap_or_default();
                    (0..7)
                        .filter(move |i| days.has(*i))
                        .map(move |i| (format!("{prefix}_day{i}"), "on".to_string()))
                })
                .collect(),
        }
    }
}

/// Saves the answers and, when they suggest an activity level and none is
/// set in Goals yet, uses it there.
pub async fn save(db: &SqlitePool, h: &Habits) -> AppResult<()> {
    let mut tx = db.begin().await?;
    sqlx::query(
        "UPDATE profile SET usual_steps = ?, plays_sport = ?, sport_key = ?, sport_sessions = ?,
             sport_minutes = ?, goes_to_gym = ?, gym_sessions = ?, gym_minutes = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
         WHERE id = 1",
    )
    .bind(h.steps.map(UsualSteps::as_str))
    .bind(h.plays_sport)
    .bind(h.sport.map(|s| s.sport.key))
    .bind(h.sport.map(|s| s.sessions_per_week))
    .bind(h.sport.map(|s| s.minutes))
    .bind(h.goes_to_gym)
    .bind(h.gym.map(|s| s.sessions_per_week))
    .bind(h.gym.map(|s| s.minutes))
    .execute(&mut *tx)
    .await?;
    if let Some(a) = suggested_activity(h) {
        sqlx::query(
            "UPDATE goal_settings SET activity = ?,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
             WHERE id = 1 AND activity IS NULL",
        )
        .bind(a.as_str())
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Adds the gym and the sport as activities repeating every week on their
/// days from `start`, so the energy they use counts in each of those days'
/// totals, as for any recorded activity. One already repeating for the same
/// sport, e.g. from a form sent twice, is left alone.
pub async fn plan(db: &SqlitePool, h: &Habits, start: Date) -> AppResult<()> {
    for habit in [h.gym, h.sport].into_iter().flatten() {
        if habit.days.is_empty() {
            continue;
        }
        let planned: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM activities
                WHERE sport_key = ? AND weekdays <> 0 AND end_date IS NULL)",
        )
        .bind(habit.sport.key)
        .fetch_one(db)
        .await?;
        if planned {
            continue;
        }
        let a = NewActivity {
            kind: SportKind::Listed(habit.sport),
            duration_min: habit.minutes,
            note: None,
            start,
            repeat: habit.days,
            end: None,
        };
        activity::create(db, &a).await?;
    }
    Ok(())
}

/// The sport given at setup, with its usual session length. Offered first
/// when adding an activity.
pub async fn usual_sport(db: &SqlitePool) -> AppResult<Option<(&'static Sport, i64)>> {
    let row: Option<(Option<String>, Option<i64>)> = sqlx::query_as(
        "SELECT sport_key, sport_minutes FROM profile WHERE id = 1 AND plays_sport = 1",
    )
    .fetch_optional(db)
    .await?;
    Ok(row.and_then(|(key, minutes)| Some((sports::find(key.as_deref()?)?, minutes?))))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sport(key: &str, sessions_per_week: i64, minutes: i64) -> Option<SportHabit> {
        Some(SportHabit {
            sport: sports::find(key).unwrap(),
            sessions_per_week,
            minutes,
            days: Weekdays::NONE,
        })
    }

    /// A habit on the days given as digits, 0 for Monday.
    fn on(key: &str, days: &str, minutes: i64) -> Option<SportHabit> {
        let days = days
            .chars()
            .filter_map(|c| c.to_digit(10))
            .fold(Weekdays::NONE, |w, d| w.with(d as usize));
        Some(habit_on(sports::find(key).unwrap(), days, minutes))
    }

    fn level(steps: Option<UsualSteps>, sport: Option<SportHabit>) -> Option<Activity> {
        suggested_activity(&Habits {
            steps,
            plays_sport: Some(sport.is_some()),
            sport,
            ..Habits::default()
        })
    }

    #[test]
    fn nothing_answered_suggests_nothing() {
        assert_eq!(suggested_activity(&Habits::default()), None);
    }

    #[test]
    fn walking_counts_beyond_everyday_movement() {
        assert_eq!(UsualSteps::Under5k.net_met_hours_per_week(), 0.0);
        // 2,500 extra steps a day: 25 min × 7 × 2.5 METs above rest.
        let w = UsualSteps::From5kTo10k.net_met_hours_per_week();
        assert!((w - 25.0 / 60.0 * 7.0 * 2.5).abs() < 1e-9);
        assert_eq!(
            level(Some(UsualSteps::Under5k), None),
            Some(Activity::Sedentary)
        );
        assert_eq!(
            level(Some(UsualSteps::From5kTo10k), None),
            Some(Activity::Sedentary)
        );
        assert_eq!(level(None, None), Some(Activity::Sedentary));
        assert_eq!(
            level(Some(UsualSteps::From10kTo15k), None),
            Some(Activity::Light)
        );
        assert_eq!(
            level(Some(UsualSteps::Over15k), None),
            Some(Activity::Moderate)
        );
    }

    #[test]
    fn sport_counts_by_energy_not_only_by_time() {
        // Yoga 2.5 METs: 2 × 1 h × 1.5 = 3 MET-hours.
        assert_eq!(level(None, sport("yoga", 2, 60)), Some(Activity::Sedentary));
        // Running 9 METs: 3 × 45 min × 8 = 18 MET-hours.
        assert_eq!(level(None, sport("running", 3, 45)), Some(Activity::Light));
        // 5 × 1 h × 8 = 40 MET-hours.
        assert_eq!(
            level(None, sport("running", 5, 60)),
            Some(Activity::Moderate)
        );
        // Strength training 3.5 METs: 3 × 1 h × 2.5 = 7.5 MET-hours, plus
        // about 7 for 7,500 steps a day.
        assert_eq!(
            level(
                Some(UsualSteps::From5kTo10k),
                sport("traditional_strength_training", 3, 60)
            ),
            Some(Activity::Light)
        );
    }

    #[test]
    fn the_gym_counts_alongside_a_sport() {
        // Running 3 × 45 min (18 MET-hours) and the gym 3 × 1 h at 3.5 METs
        // (7.5 MET-hours): about 25.5, one level.
        let h = Habits {
            plays_sport: Some(true),
            sport: sport("running", 3, 45),
            goes_to_gym: Some(true),
            gym: sport(GYM_SPORT, 3, 60),
            ..Habits::default()
        };
        assert_eq!(suggested_activity(&h), Some(Activity::Light));
        // Adding 4 × 1.5 h at the gym (15 MET-hours) reaches the next.
        let h = Habits {
            gym: sport(GYM_SPORT, 4, 90),
            ..h
        };
        assert_eq!(suggested_activity(&h), Some(Activity::Moderate));
        // Only answering "no" to the gym still suggests a level.
        let h = Habits {
            goes_to_gym: Some(false),
            ..Habits::default()
        };
        assert_eq!(suggested_activity(&h), Some(Activity::Sedentary));
    }

    #[test]
    fn walking_and_sport_add_up_to_at_most_extra_active() {
        // About 22 + 40 MET-hours.
        assert_eq!(
            level(Some(UsualSteps::From10kTo15k), sport("running", 5, 60)),
            Some(Activity::Active)
        );
        assert_eq!(
            level(Some(UsualSteps::Over15k), sport("running", 7, 120)),
            Some(Activity::VeryActive)
        );
    }

    fn form(pairs: &[(&str, &str)]) -> HabitsForm {
        let mut f = HabitsForm::default();
        for (k, v) in pairs {
            let v = v.to_string();
            match *k {
                "usual_steps" => f.usual_steps = v,
                "plays_sport" => f.plays_sport = v,
                "sport_key" => f.sport_key = v,
                "other_sport" => f.other_sport = v,
                "sport_minutes" => f.sport_minutes = v,
                "gym" => f.gym = v,
                "gym_minutes" => f.gym_minutes = v,
                "gym_program" => f.gym_program = v,
                "sport_days" | "gym_days" => {
                    let prefix = k.trim_end_matches("_days");
                    for c in v.chars() {
                        f.days.insert(format!("{prefix}_day{c}"), "on".into());
                    }
                }
                _ => panic!("{k}"),
            }
        }
        f
    }

    fn parse(f: &HabitsForm) -> (Habits, FieldErrors) {
        let mut errors = FieldErrors::default();
        let h = f.parse(Lang::En, &mut errors);
        (h, errors)
    }

    #[test]
    fn skipped_questions_are_not_errors() {
        let (h, errors) = parse(&form(&[("usual_steps", "unsure")]));
        assert!(errors.is_empty());
        assert!(h.is_empty());
    }

    #[test]
    fn sport_details_are_ignored_without_a_sport() {
        let (h, errors) = parse(&form(&[
            ("usual_steps", "5k_10k"),
            ("plays_sport", "no"),
            ("sport_key", "nonsense"),
        ]));
        assert!(errors.is_empty());
        assert_eq!(h.plays_sport, Some(false));
        assert_eq!(h.sport, None);
    }

    #[test]
    fn a_sport_needs_all_its_details() {
        let (h, errors) = parse(&form(&[("plays_sport", "yes"), ("sport_key", "running")]));
        assert_eq!(h.sport, None);
        assert!(!errors.has("sport_key"));
        assert!(errors.has("sport_days"));
        assert!(errors.has("sport_minutes"));

        let (_, errors) = parse(&form(&[
            ("plays_sport", "yes"),
            ("sport_key", "other"),
            ("other_sport", "nonsense"),
            ("sport_days", ""),
            ("sport_minutes", "31"),
        ]));
        assert!(errors.has("sport_key"));
        assert!(errors.has("sport_days"));
        assert!(errors.has("sport_minutes"));
    }

    #[test]
    fn the_gym_is_not_one_of_the_sports() {
        assert!(!POPULAR_SPORTS.contains(&GYM_SPORT));
        let (_, errors) = parse(&form(&[
            ("plays_sport", "yes"),
            ("sport_key", OTHER),
            ("other_sport", GYM_SPORT),
            ("sport_days", "024"),
            ("sport_minutes", "60"),
        ]));
        assert!(errors.has("sport_key"));
    }

    #[test]
    fn the_gym_needs_its_details_and_a_known_program() {
        let (h, errors) = parse(&form(&[("gym", "yes"), ("gym_days", "024")]));
        assert_eq!(h.gym, None);
        assert!(!errors.has("gym_days"));
        assert!(errors.has("gym_minutes"));

        let f = form(&[
            ("gym", "yes"),
            ("gym_days", "024"),
            ("gym_minutes", "60"),
            ("gym_program", "ppl"),
        ]);
        let (h, errors) = parse(&f);
        assert!(errors.is_empty());
        assert_eq!(h.goes_to_gym, Some(true));
        assert_eq!(h.gym, on(GYM_SPORT, "024", 60));
        assert_eq!(h.gym.map(|g| g.sessions_per_week), Some(3));
        let mut errors = FieldErrors::default();
        assert_eq!(f.starter(Lang::En, &mut errors).map(|s| s.key), Some("ppl"));
        assert_eq!(parse(&HabitsForm::from_habits(&h)).0, h);

        for skipped in ["", NO_PROGRAM] {
            let f = form(&[("gym_program", skipped)]);
            assert!(f.starter(Lang::En, &mut errors).is_none());
        }
        assert!(errors.is_empty());
        form(&[("gym_program", "nonsense")]).starter(Lang::En, &mut errors);
        assert!(errors.has("gym_program"));

        // Details are ignored after "no".
        let (h, errors) = parse(&form(&[("gym", "no"), ("gym_days", "0")]));
        assert!(errors.is_empty());
        assert_eq!((h.goes_to_gym, h.gym), (Some(false), None));
    }

    #[test]
    fn invalid_answers_are_rejected() {
        let (_, errors) = parse(&form(&[("usual_steps", "lots"), ("plays_sport", "maybe")]));
        assert!(errors.has("usual_steps"));
        assert!(errors.has("plays_sport"));
    }

    #[test]
    fn other_sports_round_trip_through_the_form() {
        let f = form(&[
            ("usual_steps", "10k_15k"),
            ("plays_sport", "yes"),
            ("sport_key", "other"),
            ("other_sport", "climbing"),
            ("sport_days", "13"),
            ("sport_minutes", "90"),
        ]);
        let (h, errors) = parse(&f);
        assert!(errors.is_empty());
        assert_eq!(h.sport, on("climbing", "13", 90));
        let back = HabitsForm::from_habits(&h);
        assert_eq!(back.sport_key, OTHER);
        assert_eq!(back.other_sport, "climbing");
        assert_eq!(parse(&back).0, h);

        let (h, _) = parse(&form(&[
            ("plays_sport", "yes"),
            ("sport_key", "running"),
            ("sport_days", "024"),
            ("sport_minutes", "45"),
        ]));
        assert_eq!(HabitsForm::from_habits(&h).sport_key, "running");
    }
}
