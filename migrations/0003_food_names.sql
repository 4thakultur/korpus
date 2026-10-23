-- Official product names per interface language, as published by the
-- provider a food was imported from (never machine translations). A food is
-- shown with its name in the interface language when one exists here, and
-- with `foods.name` otherwise.
CREATE TABLE food_names (
    food_id INTEGER NOT NULL REFERENCES foods (id) ON DELETE CASCADE,
    lang    TEXT    NOT NULL CHECK (lang IN ('en', 'fr', 'es')),
    name    TEXT    NOT NULL CHECK (length(name) BETWEEN 1 AND 200),
    PRIMARY KEY (food_id, lang)
) WITHOUT ROWID;
