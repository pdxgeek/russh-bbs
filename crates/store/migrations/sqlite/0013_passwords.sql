-- M3-D: member passwords (D28, D81). Forward-only: once merged this file is never edited (see
-- src/sqlite/tests.rs). Numbered 0013 after the other M3 slices' migrations.
--
-- One row per account that set a password. No row means "no password", so a member who never
-- sets one cannot be logged into with any password at all. The row holds an argon2id hash in
-- the PHC string format (the cost parameters and the salt travel in the string, so they can be
-- raised later) and nothing else: never the password. It is a separate table so no query for a
-- member's data, a profile or a list can return it by accident.
--
-- No `role_permission` rows are needed: `account.set_password` has been held by `member` and
-- above since 0003.
CREATE TABLE account_password (
    account_id INTEGER NOT NULL PRIMARY KEY REFERENCES accounts (id) ON DELETE CASCADE,
    hash       TEXT    NOT NULL CHECK (length(hash) BETWEEN 1 AND 255 AND substr(hash, 1, 10) = '$argon2id$'),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;
