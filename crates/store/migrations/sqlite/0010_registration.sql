-- M2-G: registration modes `apply` and `open` (D73, D74). Forward-only: once merged this file
-- is never edited (see src/sqlite/tests.rs). Numbered 0010; 0008 is reserved for M2-B2.
--
-- Minimal data (docs/threat-model.md, "Invites, registration, and abuse"): an application is a
-- row in M2-E's `invite_request` table and holds exactly what an invite request holds; the one
-- new column says which form it came from. The `open` cap needs one counter row per UTC day.

-- Two new waiting-room actions. `membership.apply` (the application form) has existed since
-- 0003 and is held by `waiting` and `sysop`; `membership.open` (the create-account form) is new.
-- `sysop` always holds every permission (a store test fails without its row); `waiting` is the
-- role an unknown key gets. Which form a key sees is the registration mode's choice; the
-- permission says the key may use it.
INSERT INTO role_permission (role, permission) VALUES
    ('waiting', 'membership.open'),
    ('sysop',   'membership.open');

-- Which form a queued request came from. Existing rows are invite requests.
ALTER TABLE invite_request
    ADD COLUMN kind TEXT NOT NULL DEFAULT 'invite' CHECK (kind IN ('invite', 'application'));

-- How many accounts `open` registration made on each UTC day (`day` = Unix seconds / 86400,
-- the same day number as `limit_usage`). `capped_at` is set the first time the cap refused
-- someone that day, so the audit row `registration.capped` is written once per day. Rows older
-- than a week are deleted by the next account creation; nothing here names a person.
CREATE TABLE registration_day (
    day       INTEGER PRIMARY KEY,
    opened    INTEGER NOT NULL CHECK (opened >= 0),
    capped_at INTEGER
) STRICT;
