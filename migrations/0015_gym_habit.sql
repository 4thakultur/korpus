-- The gym, answered at first-run setup apart from sports (see `habits`):
-- whether the user trains at the gym, how many times a week and for how
-- long. Only used to suggest the activity level in Goals. All optional.
ALTER TABLE profile ADD COLUMN goes_to_gym INTEGER
    CHECK (goes_to_gym IS NULL OR goes_to_gym IN (0, 1));
ALTER TABLE profile ADD COLUMN gym_sessions INTEGER
    CHECK (gym_sessions IS NULL OR gym_sessions BETWEEN 1 AND 7);
ALTER TABLE profile ADD COLUMN gym_minutes INTEGER
    CHECK (gym_minutes IS NULL OR gym_minutes BETWEEN 1 AND 1440);
