-- Usual activity, answered at first-run setup (see `habits`): roughly how
-- many steps a day and, if the user plays a sport, which one, how many times
-- a week and for how long. Only used to suggest the activity level in Goals
-- and the sport offered first in Activity. All optional.
ALTER TABLE profile ADD COLUMN usual_steps TEXT
    CHECK (usual_steps IS NULL OR usual_steps IN ('under_5k', '5k_10k', '10k_15k', 'over_15k'));
ALTER TABLE profile ADD COLUMN plays_sport INTEGER
    CHECK (plays_sport IS NULL OR plays_sport IN (0, 1));
ALTER TABLE profile ADD COLUMN sport_key TEXT;
ALTER TABLE profile ADD COLUMN sport_sessions INTEGER
    CHECK (sport_sessions IS NULL OR sport_sessions BETWEEN 1 AND 7);
ALTER TABLE profile ADD COLUMN sport_minutes INTEGER
    CHECK (sport_minutes IS NULL OR sport_minutes BETWEEN 1 AND 1440);
