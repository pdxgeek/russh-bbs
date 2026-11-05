-- FIX-UI-7a: the member's border choice may now be `blocks` (block-element borders that fill the
-- cell, for terminals that draw box lines from the font and leave gaps with extra line spacing).
-- Forward-only: earlier migrations are untouched, and once merged this file is never edited.
ALTER TABLE terminal_settings DROP CONSTRAINT IF EXISTS terminal_settings_borders_check;
ALTER TABLE terminal_settings ADD CONSTRAINT terminal_settings_borders_check
    CHECK (borders IN ('theme', 'ascii', 'blocks'));
