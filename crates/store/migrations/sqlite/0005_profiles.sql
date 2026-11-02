-- M2-F: member profiles.
-- Forward-only: earlier migrations are untouched, and once merged this file is never edited.
--
-- One row per account, written the first time the member (or staff with `members.edit`) saves
-- a profile. No row means an empty profile. Five short text fields and nothing else: no
-- avatar, no custom fields. The length caps and the sanitizing are enforced in core
-- (`core::profiles`), so widening a cap never needs a migration; text is sanitized on write and
-- again on read. Nothing here is private: every member can read it, and so can the sysop.
--
-- No `role_permission` rows are needed: `account.edit_profile` and `members.view` are already
-- held by `member` and above, and `members.edit` by `cosysop` and `sysop` (migration 0003).
CREATE TABLE profiles (
    account_id   INTEGER NOT NULL PRIMARY KEY REFERENCES accounts (id) ON DELETE CASCADE,
    display_name TEXT    NOT NULL,
    pronouns     TEXT    NOT NULL,
    location     TEXT    NOT NULL,
    about        TEXT    NOT NULL,
    website      TEXT    NOT NULL,
    updated_at   INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;
