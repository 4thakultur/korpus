-- "No target" becomes its own value, so an unset `target_source` (NULL) can
-- mean "never chosen", which the goals page treats as the estimate.
-- SQLite cannot change a CHECK constraint in place, so the single-row table
-- is rebuilt with the same columns and data.
CREATE TABLE goal_settings_new (
    id                    INTEGER PRIMARY KEY CHECK (id = 1),
    daily_kcal_target     REAL CHECK (daily_kcal_target IS NULL OR daily_kcal_target > 0),
    target_source         TEXT CHECK (target_source IS NULL OR target_source IN ('estimate', 'custom', 'none')),
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
    updated_at            TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    macro_source          TEXT
                          CHECK (macro_source IS NULL OR macro_source IN ('estimate', 'custom'))
);

INSERT INTO goal_settings_new (
    id, daily_kcal_target, target_source, goal, sex, birth_date, height_cm, weight_kg,
    activity, special_circumstances, maintenance_kcal, calculation_version,
    protein_goal_g, carbs_goal_g, fat_goal_g, updated_at, macro_source
)
SELECT
    id, daily_kcal_target, target_source, goal, sex, birth_date, height_cm, weight_kg,
    activity, special_circumstances, maintenance_kcal, calculation_version,
    protein_goal_g, carbs_goal_g, fat_goal_g, updated_at, macro_source
FROM goal_settings;

DROP TABLE goal_settings;
ALTER TABLE goal_settings_new RENAME TO goal_settings;
