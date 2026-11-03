-- M3-D: the Postgres counterpart of the SQLite 0013 migration (member passwords, D28, D81).
-- Forward-only: earlier migrations are untouched, and once merged this file is never edited.
--
-- One row per account that set a password; no row means no password. The row holds an argon2id
-- hash in the PHC string format and never the password. A separate table keeps it out of every
-- query for a member's data. `account.set_password` has been held by `member` and above since
-- 0003, so no `role_permission` rows are needed.
CREATE TABLE account_password (
    account_id BIGINT NOT NULL PRIMARY KEY REFERENCES accounts (id) ON DELETE CASCADE,
    hash       TEXT   NOT NULL CHECK (length(hash) BETWEEN 1 AND 255 AND substr(hash, 1, 10) = '$argon2id$'),
    updated_at BIGINT NOT NULL DEFAULT (extract(epoch FROM now())::bigint)
);
