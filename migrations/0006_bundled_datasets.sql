-- Release of a table shipped with Korpus that this table holds; NULL when
-- the administrator imported the publisher's own file.
ALTER TABLE datasets ADD COLUMN bundled_version TEXT;

-- The release of each bundled table last offered to this installation.
-- Startup installs a bundled release once: a table the administrator
-- imported is never replaced, and one the user removed stays removed.
CREATE TABLE bundled_datasets (
    id      TEXT PRIMARY KEY CHECK (id IN ('usda', 'ciqual', 'cofid', 'afcd', 'cnf')),
    version TEXT NOT NULL
);
