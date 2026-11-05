-- UI-10: a member's widget lists, one row per layout they changed. `widgets` is the comma-joined
-- ordered widget ids; '' means the member switched every widget off; no row means "follow the
-- default". Forward-only: earlier migrations are untouched, and once merged this file is never
-- edited.
CREATE TABLE widget_list (
    account_id INTEGER NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    layout     TEXT    NOT NULL CHECK (layout IN
                   ('classic', 'lightbar', 'compact', 'dashboard', 'wide', 'focus', 'split')),
    widgets    TEXT    NOT NULL CHECK (length(widgets) <= 400 AND widgets NOT GLOB '*[^a-z0-9_,]*'),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (account_id, layout)
) STRICT;
