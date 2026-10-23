-- Country of residence (ISO 3166-1 alpha-2). NULL until the user picks one.
-- It only decides which local food composition table search uses.
ALTER TABLE profile ADD COLUMN country TEXT
    CHECK (country IS NULL OR (length(country) = 2 AND country GLOB '[A-Z][A-Z]'));

-- National food composition tables imported from files the administrator
-- downloaded from the publisher. One row per imported table; importing a
-- table again replaces its foods.
CREATE TABLE datasets (
    id          TEXT    PRIMARY KEY CHECK (id IN ('usda', 'ciqual', 'cofid', 'afcd', 'cnf')),
    food_count  INTEGER NOT NULL CHECK (food_count >= 0),
    imported_at TEXT    NOT NULL
);

-- Reference foods from those tables. They are read-only and are searched
-- separately from the user's foods; picking one copies it into `foods`
-- (source = dataset id, provider_id = code), like a provider product.
-- Nutrients are per 100 g or per 100 ml as given by `basis`, as published,
-- except for the conversions recorded in the flag columns.
CREATE TABLE dataset_foods (
    dataset             TEXT    NOT NULL REFERENCES datasets (id) ON DELETE CASCADE,
    code                TEXT    NOT NULL CHECK (length(code) BETWEEN 1 AND 64),
    -- Name in the table's own language; name_xx only when published in xx.
    name                TEXT    NOT NULL CHECK (length(name) BETWEEN 1 AND 200),
    name_en             TEXT,
    name_fr             TEXT,
    name_es             TEXT,
    basis               TEXT    NOT NULL CHECK (basis IN ('g', 'ml')),
    energy_kcal         REAL,
    protein_g           REAL,
    carbs_g             REAL,
    fat_g               REAL,
    fiber_g             REAL,
    sugars_g            REAL,
    sat_fat_g           REAL,
    salt_g              REAL,
    -- Energy was published in kJ only and converted (1 kcal = 4.184 kJ).
    energy_from_kj      INTEGER NOT NULL DEFAULT 0 CHECK (energy_from_kj IN (0, 1)),
    -- Salt was derived from sodium (salt = sodium x 2.5).
    salt_from_sodium    INTEGER NOT NULL DEFAULT 0 CHECK (salt_from_sodium IN (0, 1)),
    -- Carbohydrate is "total, by difference" and includes fibre.
    carbs_include_fiber INTEGER NOT NULL DEFAULT 0 CHECK (carbs_include_fiber IN (0, 1)),
    -- Bit set of nutrients published as "traces" or below a detection limit
    -- and stored as 0 (bit order: see `nutrition::Nutrient::ALL`).
    trace_mask          INTEGER NOT NULL DEFAULT 0,
    -- Bit set of nutrients whose published value was invalid and dropped.
    rejected_mask       INTEGER NOT NULL DEFAULT 0,
    search_text         TEXT    NOT NULL,
    PRIMARY KEY (dataset, code)
) WITHOUT ROWID;
