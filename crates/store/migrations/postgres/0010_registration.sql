-- M2-G: the Postgres counterpart of SQLite's 0010 (D24: one migration set per backend). Same
-- tables and constraints, Postgres-native types. Forward-only: once merged this file is never
-- edited (see src/postgres/tests.rs). Numbered 0010; 0008 is reserved for M2-B2.
--
-- Minimal data (docs/threat-model.md, "Invites, registration, and abuse"): an application is a
-- row in M2-E's `invite_request` table and holds exactly what an invite request holds; the one
-- new column says which form it came from. The `open` cap needs one counter row per UTC day.

-- The new waiting-room permission `membership.open` (`membership.apply` exists since 0003).
-- `sysop` always holds every permission; `waiting` is the role an unknown key gets.
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
    day       BIGINT PRIMARY KEY,
    opened    BIGINT NOT NULL CHECK (opened >= 0),
    capped_at BIGINT
);
