-- A sport session is either one of the listed sports (`sport_key`, with its
-- English name kept in `sport`) or a custom sport named in `sport`, with an
-- intensity for its energy estimate. Sessions recorded before this have
-- neither and get no estimate until edited.
ALTER TABLE activities ADD COLUMN sport_key TEXT
    CHECK (sport_key IS NULL OR length(sport_key) BETWEEN 1 AND 40);
ALTER TABLE activities ADD COLUMN intensity TEXT
    CHECK (intensity IS NULL OR (sport_key IS NULL AND intensity IN ('light', 'moderate', 'vigorous')));
