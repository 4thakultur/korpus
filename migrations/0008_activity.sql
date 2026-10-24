-- Activity: steps walked and sport sessions, entered by hand.

-- Steps assumed for a day the user did not enter, shown as an assumption.
ALTER TABLE profile ADD COLUMN default_steps INTEGER NOT NULL DEFAULT 5000
    CHECK (default_steps BETWEEN 0 AND 100000);

-- Steps entered for a calendar date. A date without a row uses the default.
CREATE TABLE daily_steps (
    step_date   TEXT    PRIMARY KEY CHECK (step_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    steps       INTEGER NOT NULL CHECK (steps BETWEEN 0 AND 100000),
    updated_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
) WITHOUT ROWID;

-- Sport sessions. With `weekdays` = 0 an activity happens once, on
-- `start_date`. Otherwise it repeats every week on the days in `weekdays`
-- (bit 0 = Monday … bit 6 = Sunday), from `start_date` until `end_date`
-- (inclusive) or with no end.
CREATE TABLE activities (
    id            INTEGER PRIMARY KEY,
    sport         TEXT    NOT NULL CHECK (length(sport) BETWEEN 1 AND 80),
    duration_min  INTEGER NOT NULL CHECK (duration_min BETWEEN 1 AND 1440),
    note          TEXT,
    start_date    TEXT    NOT NULL CHECK (start_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    weekdays      INTEGER NOT NULL DEFAULT 0 CHECK (weekdays BETWEEN 0 AND 127),
    end_date      TEXT    CHECK (end_date IS NULL OR (weekdays <> 0 AND end_date >= start_date)),
    created_at    TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at    TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE INDEX activities_start_idx ON activities (start_date);

-- Days on which a repeating activity did not take place.
CREATE TABLE activity_skips (
    activity_id INTEGER NOT NULL REFERENCES activities (id) ON DELETE CASCADE,
    skip_date   TEXT    NOT NULL CHECK (skip_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    PRIMARY KEY (activity_id, skip_date)
) WITHOUT ROWID;
