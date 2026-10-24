-- Steps are no longer assumed for days without an entry; instead the
-- profile holds a daily step goal, which fills the steps ring at 100%.
ALTER TABLE profile DROP COLUMN default_steps;
ALTER TABLE profile ADD COLUMN step_goal INTEGER NOT NULL DEFAULT 10000
    CHECK (step_goal BETWEEN 1000 AND 100000);
