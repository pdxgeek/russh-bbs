-- UI-9: whether the member lets the board turn the terminal's mouse reporting on. Off by
-- default: mouse reporting takes over the terminal's own selection and paste, so it is the
-- member's choice.
-- Forward-only: earlier migrations are untouched, and once merged this file is never edited.
ALTER TABLE terminal_settings ADD COLUMN mouse INTEGER NOT NULL DEFAULT 0 CHECK (mouse IN (0, 1));
