//! Starter programs: common splits that load as ordinary, editable
//! programs. Routine and program names are created in the interface
//! language and are the user's to rename afterwards.

use sqlx::SqlitePool;

use super::exercises;
use super::{
    ExerciseRef, ItemInput, Schedule, Target, insert_item, insert_program, insert_routine,
};
use crate::activity::Weekdays;
use crate::error::{AppError, AppResult};
use crate::i18n::Lang;

/// One exercise of a starter routine: key, sets, reps (min, max), rest in seconds.
type Slot = (&'static str, i64, i64, Option<i64>, i64);

pub struct Starter {
    pub key: &'static str,
    /// (routine name key, weekdays as 0 = Monday …, exercises)
    routines: &'static [(&'static str, &'static [usize], &'static [Slot])],
}

impl Starter {
    pub fn name(&self, lang: Lang) -> &'static str {
        match self.key {
            "ppl" => lang.t("Push / Pull / Legs"),
            "upper_lower" => lang.t("Upper / Lower"),
            "full_body" => lang.t("Full body"),
            _ => "5×5",
        }
    }

    pub fn description(&self, lang: Lang) -> &'static str {
        match self.key {
            "ppl" => lang.t("Chest, shoulders and triceps; back and biceps; legs. 3 days a week."),
            "upper_lower" => lang.t("Upper body and lower body, each twice a week. 4 days a week."),
            "full_body" => {
                lang.t("The whole body in every session, three variations. 3 days a week.")
            }
            _ => lang.t("Five sets of five on the main barbell lifts. 3 days a week."),
        }
    }

    pub fn days_per_week(&self) -> usize {
        self.routines.iter().map(|(_, d, _)| d.len()).sum()
    }
}

fn routine_name(key: &str, lang: Lang) -> &'static str {
    match key {
        "push" => lang.t("Push"),
        "pull" => lang.t("Pull"),
        "legs" => lang.t("Legs"),
        "upper_a" => lang.t("Upper body A"),
        "upper_b" => lang.t("Upper body B"),
        "lower_a" => lang.t("Lower body A"),
        "lower_b" => lang.t("Lower body B"),
        "full_a" => lang.t("Full body A"),
        "full_b" => lang.t("Full body B"),
        "full_c" => lang.t("Full body C"),
        "five_a" => lang.t("Workout A"),
        _ => lang.t("Workout B"),
    }
}

const PPL: &[(&str, &[usize], &[Slot])] = &[
    (
        "push",
        &[0],
        &[
            ("barbell_bench_press", 4, 6, Some(8), 150),
            ("dumbbell_incline_bench_press", 3, 8, Some(12), 120),
            ("seated_dumbbell_press", 3, 8, Some(12), 120),
            ("dumbbell_lateral_raise", 3, 12, Some(15), 60),
            ("triceps_pushdown", 3, 10, Some(12), 60),
            ("chest_dip", 3, 8, Some(12), 90),
        ],
    ),
    (
        "pull",
        &[2],
        &[
            ("lat_pulldown", 4, 8, Some(10), 120),
            ("barbell_row", 4, 6, Some(8), 150),
            ("seated_cable_row", 3, 10, Some(12), 90),
            ("face_pull", 3, 12, Some(15), 60),
            ("barbell_curl", 3, 8, Some(12), 60),
            ("hammer_curl", 3, 10, Some(12), 60),
        ],
    ),
    (
        "legs",
        &[4],
        &[
            ("back_squat", 4, 6, Some(8), 180),
            ("romanian_deadlift", 3, 8, Some(10), 150),
            ("leg_press", 3, 10, Some(12), 120),
            ("leg_extension", 3, 12, Some(15), 60),
            ("lying_leg_curl", 3, 10, Some(12), 60),
            ("standing_calf_raise", 4, 12, Some(15), 60),
        ],
    ),
];

const UPPER_LOWER: &[(&str, &[usize], &[Slot])] = &[
    (
        "upper_a",
        &[0],
        &[
            ("barbell_bench_press", 4, 6, Some(8), 150),
            ("barbell_row", 4, 6, Some(8), 150),
            ("seated_dumbbell_press", 3, 8, Some(10), 120),
            ("lat_pulldown", 3, 8, Some(10), 120),
            ("barbell_curl", 2, 10, Some(12), 60),
            ("triceps_pushdown", 2, 10, Some(12), 60),
        ],
    ),
    (
        "lower_a",
        &[1],
        &[
            ("back_squat", 4, 6, Some(8), 180),
            ("romanian_deadlift", 3, 8, Some(10), 150),
            ("leg_press", 3, 10, Some(12), 120),
            ("lying_leg_curl", 3, 10, Some(12), 60),
            ("standing_calf_raise", 4, 10, Some(15), 60),
        ],
    ),
    (
        "upper_b",
        &[3],
        &[
            ("overhead_press", 4, 6, Some(8), 150),
            ("pull_up", 4, 6, Some(10), 150),
            ("dumbbell_incline_bench_press", 3, 8, Some(12), 120),
            ("seated_cable_row", 3, 10, Some(12), 90),
            ("dumbbell_lateral_raise", 3, 12, Some(15), 60),
            ("hammer_curl", 2, 10, Some(12), 60),
        ],
    ),
    (
        "lower_b",
        &[4],
        &[
            ("deadlift", 3, 5, None, 180),
            ("bulgarian_split_squat", 3, 8, Some(12), 90),
            ("leg_extension", 3, 12, Some(15), 60),
            ("seated_leg_curl", 3, 10, Some(12), 60),
            ("seated_calf_raise", 4, 12, Some(15), 60),
        ],
    ),
];

const FULL_BODY: &[(&str, &[usize], &[Slot])] = &[
    (
        "full_a",
        &[0],
        &[
            ("back_squat", 3, 6, Some(8), 150),
            ("barbell_bench_press", 3, 6, Some(8), 150),
            ("barbell_row", 3, 8, Some(10), 120),
            ("lying_leg_curl", 3, 10, Some(12), 60),
            ("dumbbell_lateral_raise", 2, 12, Some(15), 60),
        ],
    ),
    (
        "full_b",
        &[2],
        &[
            ("romanian_deadlift", 3, 8, Some(10), 150),
            ("overhead_press", 3, 6, Some(8), 150),
            ("lat_pulldown", 3, 8, Some(12), 120),
            ("leg_press", 3, 10, Some(12), 120),
            ("barbell_curl", 2, 10, Some(12), 60),
        ],
    ),
    (
        "full_c",
        &[4],
        &[
            ("front_squat", 3, 6, Some(8), 150),
            ("dumbbell_incline_bench_press", 3, 8, Some(12), 120),
            ("seated_cable_row", 3, 10, Some(12), 90),
            ("hip_thrust", 3, 8, Some(12), 90),
            ("triceps_pushdown", 2, 10, Some(12), 60),
        ],
    ),
];

// Workout A and B alternate (A, B, A, then B, A, B): a rotation.
const FIVE_BY_FIVE: &[(&str, &[usize], &[Slot])] = &[
    (
        "five_a",
        &[0, 4],
        &[
            ("back_squat", 5, 5, None, 180),
            ("barbell_bench_press", 5, 5, None, 180),
            ("barbell_row", 5, 5, None, 180),
        ],
    ),
    (
        "five_b",
        &[2],
        &[
            ("back_squat", 5, 5, None, 180),
            ("overhead_press", 5, 5, None, 180),
            ("deadlift", 1, 5, None, 180),
        ],
    ),
];

pub const STARTERS: &[Starter] = &[
    Starter {
        key: "ppl",
        routines: PPL,
    },
    Starter {
        key: "upper_lower",
        routines: UPPER_LOWER,
    },
    Starter {
        key: "full_body",
        routines: FULL_BODY,
    },
    Starter {
        key: "5x5",
        routines: FIVE_BY_FIVE,
    },
];

pub fn find(key: &str) -> Option<&'static Starter> {
    STARTERS.iter().find(|s| s.key == key)
}

fn schedule_of(s: &Starter) -> Schedule {
    if s.key == "5x5" {
        Schedule::Rotation
    } else {
        Schedule::Weekly
    }
}

/// Creates a program from a starter, all at once or not at all.
pub async fn create(db: &SqlitePool, s: &Starter, name: &str, lang: Lang) -> AppResult<i64> {
    let mut tx = db.begin().await?;
    let schedule = schedule_of(s);
    let program_id = insert_program(&mut tx, name, schedule).await?;
    for (key, days, slots) in s.routines {
        let days = match schedule {
            Schedule::Weekly => days.iter().fold(Weekdays::NONE, |w, d| w.with(*d)),
            Schedule::Rotation => Weekdays::NONE,
        };
        let routine_id = insert_routine(&mut tx, program_id, routine_name(key, lang), days).await?;
        for (ex, sets, min, max, rest) in *slots {
            let exercise = exercises::find(ex)
                .ok_or_else(|| AppError::Internal(format!("starter exercise {ex} missing")))?;
            let item = ItemInput {
                sets: *sets,
                target: Target::Reps {
                    min: *min,
                    max: *max,
                },
                weight_kg: None,
                rest_s: Some(*rest),
                note: None,
            };
            insert_item(&mut tx, routine_id, &ExerciseRef::Builtin(exercise), &item).await?;
        }
    }
    tx.commit().await?;
    Ok(program_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starters_use_listed_exercises_and_valid_plans() {
        for s in STARTERS {
            assert!(s.days_per_week() >= 3, "{}", s.key);
            for (_, days, slots) in s.routines {
                assert!(days.iter().all(|d| *d < 7));
                for (ex, sets, min, max, rest) in *slots {
                    let e = exercises::find(ex).unwrap_or_else(|| panic!("{ex}"));
                    assert!(!e.timed, "{ex}");
                    assert!((1..=super::super::MAX_SETS).contains(sets));
                    assert!(max.is_none_or(|m| m > *min));
                    assert!((0..=super::super::MAX_REST_S).contains(rest));
                }
            }
        }
    }

    #[tokio::test]
    async fn a_starter_creates_an_active_program_with_its_routines() {
        let db = crate::db::connect_memory().await.unwrap();
        let ppl = find("ppl").unwrap();
        let id = create(&db, ppl, ppl.name(Lang::Fr), Lang::Fr)
            .await
            .unwrap();
        let p = super::super::program(&db, id).await.unwrap().unwrap();
        assert!(p.active);
        assert_eq!(p.name, "Poussée / Tirage / Jambes");
        let routines = super::super::routines(&db, id).await.unwrap();
        let names: Vec<&str> = routines.iter().map(|r| r.routine.name.as_str()).collect();
        assert_eq!(names, ["Poussée", "Tirage", "Jambes"]);
        assert_eq!(routines[0].exercises, 6);
        assert!(routines[1].routine.days().has(2));

        // A second program is not made active.
        let five = find("5x5").unwrap();
        let id2 = create(&db, five, "5×5", Lang::En).await.unwrap();
        let p2 = super::super::program(&db, id2).await.unwrap().unwrap();
        assert!(!p2.active);
        assert_eq!(p2.schedule(), Schedule::Rotation);
        let r = super::super::routines(&db, id2).await.unwrap();
        assert!(r.iter().all(|r| r.routine.days().is_empty()));
    }
}
