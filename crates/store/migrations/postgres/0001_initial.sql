-- M1-G: the Postgres counterpart of the SQLite 0001 migration (D24: one migration set per
-- backend). Same tables, same constraints, Postgres-native types.
-- Migrations are forward-only: once merged this file is never edited (see src/postgres/tests.rs).
-- Only fingerprints and public metadata are stored. No secrets, tokens or private keys.

CREATE TABLE accounts (
    -- Identity column (the native form of BIGSERIAL). Ids are never reused (D35).
    id                BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    username          TEXT   NOT NULL,
    -- ASCII confusables skeleton of `username` (D17): the uniqueness key.
    username_skeleton TEXT   NOT NULL UNIQUE,
    role              TEXT   NOT NULL CHECK (role IN ('member', 'sysop')),
    state             TEXT   NOT NULL CHECK (state IN ('pending', 'approved', 'active', 'suspended')),
    -- Integer Unix seconds set by the database, as in SQLite (D35).
    created_at        BIGINT NOT NULL DEFAULT (extract(epoch FROM now())::bigint)
);

-- One row per key. A fingerprint is the primary key, so no two accounts can share a key.
CREATE TABLE account_keys (
    fingerprint TEXT   NOT NULL PRIMARY KEY,
    account_id  BIGINT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    created_at  BIGINT NOT NULL DEFAULT (extract(epoch FROM now())::bigint)
);

CREATE INDEX account_keys_by_account ON account_keys (account_id);

CREATE TABLE settings (
    key   TEXT NOT NULL PRIMARY KEY,
    value TEXT NOT NULL
);

-- Quiet by default (D2/D23).
INSERT INTO settings (key, value) VALUES ('registration_mode', 'invite');
