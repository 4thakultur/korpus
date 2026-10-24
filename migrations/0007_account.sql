-- The login protecting the installation. Korpus has one account (id = 1),
-- created in the browser with a setup code printed in the server log.
CREATE TABLE account (
    id                  INTEGER PRIMARY KEY CHECK (id = 1),
    username            TEXT    NOT NULL CHECK (length(username) BETWEEN 1 AND 64),
    -- Argon2id hash in PHC string format; the password itself is never stored.
    password_hash       TEXT    NOT NULL,
    -- Base32 secret of the authenticator app, once two-factor login is on.
    totp_secret         TEXT,
    -- Secret shown while two-factor login is being set up, until a first
    -- code from the app confirms it.
    totp_pending_secret TEXT,
    -- Last accepted 30-second time step, so a code cannot be used twice.
    totp_last_step      INTEGER,
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

-- Browser sessions. Only the SHA-256 hash of each token is stored, so a copy
-- of the database (such as a backup) cannot be used to log in.
CREATE TABLE sessions (
    token_hash  TEXT    PRIMARY KEY CHECK (length(token_hash) = 64),
    -- The password was checked; the authenticator code is still missing.
    mfa_pending INTEGER NOT NULL DEFAULT 0 CHECK (mfa_pending IN (0, 1)),
    -- Unix seconds.
    created_at  INTEGER NOT NULL,
    expires_at  INTEGER NOT NULL
) WITHOUT ROWID;

CREATE INDEX sessions_expiry_idx ON sessions (expires_at);
