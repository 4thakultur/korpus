-- Interface language. NULL follows the browser's Accept-Language header.
ALTER TABLE profile ADD COLUMN language TEXT
    CHECK (language IS NULL OR language IN ('en', 'fr', 'es'));
