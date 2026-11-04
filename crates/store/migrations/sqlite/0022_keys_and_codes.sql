-- FIX-M3-11: keys, device-link codes, staff recovery codes and key notices (D97). Forward-only:
-- once merged this file is never edited (see src/sqlite/tests.rs).
--
-- Keys: when a key last signed in, for the My keys list. NULL means "not since this was
-- recorded". No secret: `account_keys` has only public fingerprints.
ALTER TABLE account_keys ADD COLUMN last_used_at INTEGER;

-- A one-time code that binds a key. `code_hash` is 64 lowercase hex characters of SHA-256 over a
-- domain prefix naming the kind and the normalized code (D67's rule): the code itself exists only
-- on the screen that shows it. `link` is made by the account for itself (10 minutes); `recovery`
-- by a staff member for the account (24 hours). `redeemed_at` is set once, by the redemption
-- transaction, and `voided_at` when a newer code of the same kind replaces it; a code with either
-- set can never be used. Times are the store's clock (Unix seconds), written explicitly.
CREATE TABLE login_code (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    kind        TEXT    NOT NULL CHECK (kind IN ('link', 'recovery')),
    code_hash   TEXT    NOT NULL UNIQUE CHECK (length(code_hash) = 64),
    account_id  INTEGER NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    issued_by   INTEGER REFERENCES accounts (id) ON DELETE SET NULL,
    created_at  INTEGER NOT NULL,
    expires_at  INTEGER NOT NULL CHECK (expires_at > created_at),
    redeemed_at INTEGER,
    voided_at   INTEGER
) STRICT;
CREATE INDEX login_code_by_account ON login_code (account_id, kind, created_at);

-- What the member is told at the next sign-in: a key added or removed, a code redeemed. Public
-- facts only (the kind, when, the key's public fingerprint). `seen_at` is set when the notice is
-- shown at a sign-in.
CREATE TABLE key_notice (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id  INTEGER NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    kind        TEXT    NOT NULL CHECK (kind IN
                    ('key_added', 'key_removed', 'key_linked', 'device_linked', 'recovered')),
    fingerprint TEXT,
    at          INTEGER NOT NULL,
    seen_at     INTEGER
) STRICT;
CREATE INDEX key_notice_unseen ON key_notice (account_id, id) WHERE seen_at IS NULL;

-- The new permissions (D97). `keys.manage`: My keys and making a link code. `keys.redeem`: use a
-- code in the waiting room. `keys.recover`: issue a recovery code, for staff only. `sysop` always
-- holds every permission.
INSERT INTO role_permission (role, permission) VALUES
    ('member',    'keys.manage'),
    ('trusted',   'keys.manage'),
    ('moderator', 'keys.manage'),
    ('cosysop',   'keys.manage'),
    ('sysop',     'keys.manage'),
    ('waiting',   'keys.redeem'),
    ('sysop',     'keys.redeem'),
    ('cosysop',   'keys.recover'),
    ('sysop',     'keys.recover');
