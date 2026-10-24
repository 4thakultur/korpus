-- How the macronutrient goals were set: 'estimate' (from the calorie target,
-- goal and weight, see `goals::estimate_macros`) or 'custom'. NULL for goals
-- saved before this, which were entered by hand. With 'estimate', the stored
-- grams are for a day without recorded activity; the diary adds the energy of
-- that day's steps and sport to the carbohydrate goal.
ALTER TABLE goal_settings ADD COLUMN macro_source TEXT
    CHECK (macro_source IS NULL OR macro_source IN ('estimate', 'custom'));
