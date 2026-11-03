-- M3-B (Postgres counterpart of the SQLite 0012 migration, D24): saved drafts for the
-- full-screen message editor (D80). Forward-only: once merged this file is never edited.
--
-- One row per member per context. A context is the opaque key the calling feature chooses
-- (`forum:reply:1234`, `mail:new`); the database never interprets it. The body is text the
-- member chose to keep, already sanitized by core's shared sanitizer before it is written; it
-- is stored as the server can read it (nothing here is encrypted). The CHECKs repeat core's
-- ceilings (`MAX_BODY_CHARS`, `MAX_CONTEXT_CHARS`) so a bug in a caller cannot fill the
-- database; the cap on drafts per member is enforced by the write itself, in one transaction.
-- No permission: a draft is reachable only by its own account (the id comes from the session),
-- and a feature checks its own permission before it opens the editor.

CREATE TABLE draft (
    account_id BIGINT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    context    TEXT NOT NULL CHECK (char_length(context) BETWEEN 1 AND 64),
    body       TEXT NOT NULL CHECK (char_length(body) BETWEEN 1 AND 16000),
    updated_at BIGINT NOT NULL,
    PRIMARY KEY (account_id, context)
);

-- "Your drafts, newest first" reads one member's rows in this order.
CREATE INDEX draft_by_updated ON draft (account_id, updated_at DESC);
