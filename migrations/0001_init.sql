-- Korpus initial schema.
-- Dates are stored as ISO-8601 TEXT: calendar dates as 'YYYY-MM-DD',
-- timestamps as UTC 'YYYY-MM-DDTHH:MM:SSZ'. Nutrient values are REAL and
-- always expressed per 100 g or per 100 ml as given by the row's `basis`.

-- Single local profile (single-user self-hosting).
CREATE TABLE profile (
    id              INTEGER PRIMARY KEY CHECK (id = 1),
    setup_complete  INTEGER NOT NULL DEFAULT 0 CHECK (setup_complete IN (0, 1)),
    unit_system     TEXT    NOT NULL DEFAULT 'metric' CHECK (unit_system IN ('metric', 'imperial')),
    timezone        TEXT    NOT NULL DEFAULT 'UTC',
    created_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

-- Calorie goal and the inputs used to estimate it. All inputs are optional so
-- the user can skip the estimate and set a custom target (or no target).
CREATE TABLE goal_settings (
    id                    INTEGER PRIMARY KEY CHECK (id = 1),
    daily_kcal_target     REAL CHECK (daily_kcal_target IS NULL OR daily_kcal_target > 0),
    target_source         TEXT CHECK (target_source IS NULL OR target_source IN ('estimate', 'custom')),
    goal                  TEXT NOT NULL DEFAULT 'maintain'
                          CHECK (goal IN ('maintain', 'lose_slow', 'lose', 'gain_slow')),
    sex                   TEXT CHECK (sex IS NULL OR sex IN ('male', 'female')),
    birth_date            TEXT,
    height_cm             REAL CHECK (height_cm IS NULL OR height_cm > 0),
    weight_kg             REAL CHECK (weight_kg IS NULL OR weight_kg > 0),
    activity              TEXT CHECK (activity IS NULL OR activity IN
                          ('sedentary', 'light', 'moderate', 'active', 'very_active')),
    special_circumstances INTEGER NOT NULL DEFAULT 0 CHECK (special_circumstances IN (0, 1)),
    maintenance_kcal      REAL,
    calculation_version   INTEGER,
    protein_goal_g        REAL CHECK (protein_goal_g IS NULL OR protein_goal_g >= 0),
    carbs_goal_g          REAL CHECK (carbs_goal_g IS NULL OR carbs_goal_g >= 0),
    fat_goal_g            REAL CHECK (fat_goal_g IS NULL OR fat_goal_g >= 0),
    updated_at            TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

INSERT INTO profile (id) VALUES (1);
INSERT INTO goal_settings (id) VALUES (1);

-- Foods are shared within the installation (there is a single local profile).
-- Rows come from the user ('user') or are imported from a provider, in which
-- case `source` names the provider and `provider_id` its identifier.
CREATE TABLE foods (
    id                INTEGER PRIMARY KEY,
    name              TEXT    NOT NULL CHECK (length(name) BETWEEN 1 AND 200),
    brand             TEXT,
    barcode           TEXT,
    basis             TEXT    NOT NULL CHECK (basis IN ('g', 'ml')),
    energy_kcal       REAL,
    protein_g         REAL,
    carbs_g           REAL,
    fat_g             REAL,
    fiber_g           REAL,
    sugars_g          REAL,
    sat_fat_g         REAL,
    salt_g            REAL,
    energy_from_kj    INTEGER NOT NULL DEFAULT 0 CHECK (energy_from_kj IN (0, 1)),
    serving_amount    REAL CHECK (serving_amount IS NULL OR serving_amount > 0),
    serving_label     TEXT,
    notes             TEXT,
    source            TEXT    NOT NULL,
    provider_id       TEXT,
    source_updated_at TEXT,
    fetched_at        TEXT,
    edited_locally    INTEGER NOT NULL DEFAULT 0 CHECK (edited_locally IN (0, 1)),
    search_text       TEXT    NOT NULL,
    created_at        TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at        TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE UNIQUE INDEX foods_barcode_uq ON foods (barcode) WHERE barcode IS NOT NULL;
CREATE UNIQUE INDEX foods_provider_uq ON foods (source, provider_id) WHERE provider_id IS NOT NULL;
CREATE INDEX foods_updated_idx ON foods (updated_at);

-- Each entry keeps a snapshot of the food's name and per-100 nutrition so that
-- editing or deleting a food never rewrites past diary totals.
CREATE TABLE diary_entries (
    id          INTEGER PRIMARY KEY,
    diary_date  TEXT    NOT NULL CHECK (diary_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    meal        TEXT    NOT NULL CHECK (meal IN ('breakfast', 'lunch', 'dinner', 'snacks')),
    food_id     INTEGER REFERENCES foods (id) ON DELETE SET NULL,
    amount      REAL    NOT NULL CHECK (amount > 0),
    unit        TEXT    NOT NULL CHECK (unit IN ('g', 'ml')),
    food_name   TEXT    NOT NULL,
    food_brand  TEXT,
    source      TEXT    NOT NULL,
    energy_kcal REAL,
    protein_g   REAL,
    carbs_g     REAL,
    fat_g       REAL,
    fiber_g     REAL,
    sugars_g    REAL,
    sat_fat_g   REAL,
    salt_g      REAL,
    created_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE INDEX diary_entries_date_idx ON diary_entries (diary_date, meal);
CREATE INDEX diary_entries_food_idx ON diary_entries (food_id);

-- One weigh-in per calendar date, stored canonically in kilograms.
CREATE TABLE weight_entries (
    id          INTEGER PRIMARY KEY,
    entry_date  TEXT NOT NULL UNIQUE CHECK (entry_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    weight_kg   REAL NOT NULL CHECK (weight_kg > 0),
    note        TEXT,
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);
