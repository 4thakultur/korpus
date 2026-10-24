-- Gym programs: a program holds routines (e.g. "Push", "Pull", "Legs"),
-- and a routine holds the exercises to do with their sets and reps.

-- Exercises the user added because the built-in list (src/gym/exercises.rs)
-- does not have them. Built-in exercises are not stored; routines refer to
-- them by key.
CREATE TABLE gym_exercises (
    id          INTEGER PRIMARY KEY,
    name        TEXT    NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
    muscle      TEXT    NOT NULL CHECK (length(muscle) BETWEEN 1 AND 20),
    equipment   TEXT    NOT NULL CHECK (length(equipment) BETWEEN 1 AND 20),
    -- 1 when sets are held for a time (a plank) rather than counted in reps.
    timed       INTEGER NOT NULL DEFAULT 0 CHECK (timed IN (0, 1)),
    created_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

-- `schedule` is 'weekly' (each routine on chosen weekdays) or 'rotation'
-- (routines done in order, A, B, C, A, …, whatever the weekday). At most
-- one program is active: the one the Gym page shows first.
CREATE TABLE gym_programs (
    id          INTEGER PRIMARY KEY,
    name        TEXT    NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
    schedule    TEXT    NOT NULL DEFAULT 'weekly' CHECK (schedule IN ('weekly', 'rotation')),
    active      INTEGER NOT NULL DEFAULT 0 CHECK (active IN (0, 1)),
    created_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE UNIQUE INDEX gym_programs_one_active ON gym_programs (active) WHERE active = 1;

-- `weekdays` (bit 0 = Monday … bit 6 = Sunday) is used by weekly programs;
-- `position` orders the routines, which is the order of a rotation.
CREATE TABLE gym_routines (
    id          INTEGER PRIMARY KEY,
    program_id  INTEGER NOT NULL REFERENCES gym_programs (id) ON DELETE CASCADE,
    name        TEXT    NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
    position    INTEGER NOT NULL DEFAULT 0,
    weekdays    INTEGER NOT NULL DEFAULT 0 CHECK (weekdays BETWEEN 0 AND 127),
    created_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE INDEX gym_routines_program_idx ON gym_routines (program_id, position);

-- One exercise of a routine: a built-in exercise (`exercise_key`) or one of
-- the user's (`custom_exercise_id`). Sets are counted in reps (`reps_min`,
-- with `reps_max` for a range such as 8-12) or held for `duration_s`.
-- `weight_kg` is the planned load (added load for body-weight exercises).
-- A custom exercise cannot be deleted while a routine uses it.
CREATE TABLE gym_routine_exercises (
    id                  INTEGER PRIMARY KEY,
    routine_id          INTEGER NOT NULL REFERENCES gym_routines (id) ON DELETE CASCADE,
    position            INTEGER NOT NULL DEFAULT 0,
    exercise_key        TEXT    CHECK (exercise_key IS NULL OR length(exercise_key) BETWEEN 1 AND 40),
    custom_exercise_id  INTEGER REFERENCES gym_exercises (id) ON DELETE RESTRICT,
    sets                INTEGER NOT NULL CHECK (sets BETWEEN 1 AND 20),
    reps_min            INTEGER CHECK (reps_min IS NULL OR reps_min BETWEEN 1 AND 100),
    reps_max            INTEGER CHECK (reps_max IS NULL OR (reps_min IS NOT NULL AND reps_max > reps_min AND reps_max <= 100)),
    duration_s          INTEGER CHECK (duration_s IS NULL OR duration_s BETWEEN 1 AND 3600),
    weight_kg           REAL    CHECK (weight_kg IS NULL OR weight_kg BETWEEN 0 AND 1000),
    rest_s              INTEGER CHECK (rest_s IS NULL OR rest_s BETWEEN 0 AND 900),
    note                TEXT    CHECK (note IS NULL OR length(note) <= 200),
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    CHECK ((exercise_key IS NULL) <> (custom_exercise_id IS NULL)),
    CHECK ((reps_min IS NULL) <> (duration_s IS NULL))
);

CREATE INDEX gym_routine_exercises_routine_idx ON gym_routine_exercises (routine_id, position);
CREATE INDEX gym_routine_exercises_custom_idx ON gym_routine_exercises (custom_exercise_id)
    WHERE custom_exercise_id IS NOT NULL;
