-- UI-9: the Postgres counterpart of the SQLite 0026 migration (mouse reporting, member's choice,
-- off by default).
-- Forward-only: earlier migrations are untouched, and once merged this file is never edited.
ALTER TABLE terminal_settings ADD COLUMN mouse BOOLEAN NOT NULL DEFAULT FALSE;
