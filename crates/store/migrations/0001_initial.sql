-- M1-B: accounts, the SSH key fingerprints bound to them, and board settings.
-- Migrations are forward-only: once merged this file is never edited (see src/migrate_tests.rs).
-- Only fingerprints and public metadata are stored. No secrets, tokens or private keys.

CREATE TABLE accounts (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    username          TEXT    NOT NULL,
    -- ASCII confusables skeleton of `username` (D17): the uniqueness key.
    username_skeleton TEXT    NOT NULL UNIQUE,
    role              TEXT    NOT NULL CHECK (role IN ('member', 'sysop')),
    state             TEXT    NOT NULL CHECK (state IN ('pending', 'approved', 'active', 'suspended')),
    created_at        INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;

-- One row per key. A fingerprint is the primary key, so no two accounts can share a key.
CREATE TABLE account_keys (
    fingerprint TEXT    NOT NULL PRIMARY KEY,
    account_id  INTEGER NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    created_at  INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;

CREATE INDEX account_keys_by_account ON account_keys (account_id);

CREATE TABLE settings (
    key   TEXT NOT NULL PRIMARY KEY,
    value TEXT NOT NULL
) STRICT;

-- Quiet by default (D2/D23).
INSERT INTO settings (key, value) VALUES ('registration_mode', 'invite');
