-- UI-10: the Postgres counterpart of the SQLite 0025 migration (a member's widget lists, one row
-- per layout they changed; '' = all off, no row = follow the default).
-- Forward-only: earlier migrations are untouched, and once merged this file is never edited.
CREATE TABLE widget_list (
    account_id BIGINT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    layout     TEXT   NOT NULL CHECK (layout IN
                   ('classic', 'lightbar', 'compact', 'dashboard', 'wide', 'focus', 'split')),
    widgets    TEXT   NOT NULL CHECK (length(widgets) <= 400 AND widgets ~ '^[a-z0-9_,]*$'),
    updated_at BIGINT NOT NULL DEFAULT extract(epoch FROM now())::bigint,
    PRIMARY KEY (account_id, layout)
);
