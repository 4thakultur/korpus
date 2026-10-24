-- Workouts: what was actually done at the gym, set by set.

-- A workout is in progress while `finished_at` is NULL; only one can be.
-- `workout_date` is the calendar date it counts for (in the profile's time
-- zone), which can be changed to log a workout done earlier. `name` and the
-- planned values below are copied when it starts, so editing or deleting
-- the routine later leaves the history as it was.
CREATE TABLE gym_workouts (
    id            INTEGER PRIMARY KEY,
    workout_date  TEXT    NOT NULL CHECK (workout_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    name          TEXT    NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
    routine_id    INTEGER REFERENCES gym_routines (id) ON DELETE SET NULL,
    started_at    TEXT    NOT NULL,
    finished_at   TEXT,
    note          TEXT    CHECK (note IS NULL OR length(note) <= 500),
    created_at    TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at    TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE UNIQUE INDEX gym_workouts_one_open ON gym_workouts ((finished_at IS NULL))
    WHERE finished_at IS NULL;
CREATE INDEX gym_workouts_date_idx ON gym_workouts (workout_date, id);
CREATE INDEX gym_workouts_routine_idx ON gym_workouts (routine_id);

-- One exercise of a workout: a built-in exercise (`exercise_key`) or one of
-- the user's. `exercise_name` keeps the name it had, so a deleted custom
-- exercise (whose id becomes NULL) still reads correctly. The `target_*`
-- columns hold the routine's plan when the workout started, if any.
CREATE TABLE gym_workout_exercises (
    id                  INTEGER PRIMARY KEY,
    workout_id          INTEGER NOT NULL REFERENCES gym_workouts (id) ON DELETE CASCADE,
    position            INTEGER NOT NULL DEFAULT 0,
    exercise_key        TEXT    CHECK (exercise_key IS NULL OR length(exercise_key) BETWEEN 1 AND 40),
    custom_exercise_id  INTEGER REFERENCES gym_exercises (id) ON DELETE SET NULL,
    exercise_name       TEXT    NOT NULL CHECK (length(exercise_name) BETWEEN 1 AND 80),
    timed               INTEGER NOT NULL DEFAULT 0 CHECK (timed IN (0, 1)),
    target_sets         INTEGER CHECK (target_sets IS NULL OR target_sets BETWEEN 1 AND 20),
    target_reps_min     INTEGER CHECK (target_reps_min IS NULL OR target_reps_min BETWEEN 1 AND 100),
    target_reps_max     INTEGER CHECK (target_reps_max IS NULL OR target_reps_max BETWEEN 1 AND 100),
    target_duration_s   INTEGER CHECK (target_duration_s IS NULL OR target_duration_s BETWEEN 1 AND 3600),
    target_weight_kg    REAL    CHECK (target_weight_kg IS NULL OR target_weight_kg BETWEEN 0 AND 1000),
    rest_s              INTEGER CHECK (rest_s IS NULL OR rest_s BETWEEN 0 AND 900),
    CHECK (exercise_key IS NULL OR custom_exercise_id IS NULL)
);

CREATE INDEX gym_workout_exercises_workout_idx ON gym_workout_exercises (workout_id, position);
CREATE INDEX gym_workout_exercises_key_idx ON gym_workout_exercises (exercise_key)
    WHERE exercise_key IS NOT NULL;
CREATE INDEX gym_workout_exercises_custom_idx ON gym_workout_exercises (custom_exercise_id)
    WHERE custom_exercise_id IS NOT NULL;

-- The sets of a workout exercise. A set counts once `done` is 1; sets left
-- undone are dropped when the workout is finished. `weight_kg` is the load
-- (added load for body-weight exercises).
CREATE TABLE gym_workout_sets (
    id                   INTEGER PRIMARY KEY,
    workout_exercise_id  INTEGER NOT NULL REFERENCES gym_workout_exercises (id) ON DELETE CASCADE,
    position             INTEGER NOT NULL DEFAULT 0,
    reps                 INTEGER CHECK (reps IS NULL OR reps BETWEEN 0 AND 1000),
    duration_s           INTEGER CHECK (duration_s IS NULL OR duration_s BETWEEN 0 AND 3600),
    weight_kg            REAL    CHECK (weight_kg IS NULL OR weight_kg BETWEEN 0 AND 1000),
    done                 INTEGER NOT NULL DEFAULT 0 CHECK (done IN (0, 1))
);

CREATE INDEX gym_workout_sets_exercise_idx ON gym_workout_sets (workout_exercise_id, position);
