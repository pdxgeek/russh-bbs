-- M3-I: internal mail, stage 1 (Postgres counterpart of the SQLite 0019 migration, D24; D78, D90).
-- Forward-only: once merged this file is never edited.
--
-- A mailbox is the set of `mail_message` rows with one `recipient_id`. `subject` and `body` are
-- SEALED BYTES, not text: XChaCha20-Poly1305 under a server-held key that is never in the
-- database (it is a file beside the host key). Each is `nonce (24) || ciphertext || tag (16)`,
-- so at least 41 bytes for one character, and the upper bounds are the text limits at four UTF-8
-- bytes a character plus the 40 bytes of overhead. `key_id` says which key sealed the row so a
-- key can be rotated later (core::mail::key). The id is chosen by core, not by the database: it
-- is part of the sealed value's associated data, together with the recipient's account id, so a
-- row copied to another message or mailbox fails to open.
--
-- The rest is metadata the server can see: who sent it, when, whether it was opened. Nothing
-- here holds a subject or a text in clear. Deleting an account deletes its mailbox; a deleted
-- sender leaves the message with no sender.
CREATE TABLE mail_message (
    id           BIGINT PRIMARY KEY CHECK (id > 0),
    recipient_id BIGINT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    sender_id    BIGINT REFERENCES accounts (id) ON DELETE SET NULL,
    sent_at      BIGINT NOT NULL,
    read_at      BIGINT,
    key_id       BIGINT NOT NULL CHECK (key_id > 0),
    subject      BYTEA NOT NULL CHECK (octet_length(subject) BETWEEN 41 AND 520),
    body         BYTEA NOT NULL CHECK (octet_length(body) BETWEEN 41 AND 32040)
);

-- "A mailbox, newest first" and the per-mailbox count read this index.
CREATE INDEX mail_by_recipient ON mail_message (recipient_id, sent_at DESC, id DESC);

-- A report is what a recipient chose to send to staff: a COPY of the text, stored in clear, made
-- by the reporter. It is not the message (which stays sealed) and staff cannot check it against
-- it. `message_id` is not a foreign key: the recipient may delete the message afterwards, and
-- the report stays. One report per reporter per message; closing sets `closed_at`.
CREATE TABLE mail_report (
    id           BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    reporter_id  BIGINT REFERENCES accounts (id) ON DELETE SET NULL,
    sender_id    BIGINT REFERENCES accounts (id) ON DELETE SET NULL,
    message_id   BIGINT NOT NULL,
    reported_at  BIGINT NOT NULL,
    copy_subject TEXT NOT NULL CHECK (char_length(copy_subject) BETWEEN 1 AND 120),
    copy_body    TEXT NOT NULL CHECK (char_length(copy_body) BETWEEN 1 AND 8000),
    closed_at    BIGINT
);

CREATE UNIQUE INDEX mail_report_once ON mail_report (reporter_id, message_id)
    WHERE reporter_id IS NOT NULL;
CREATE INDEX mail_report_open ON mail_report (closed_at, reported_at DESC, id DESC);

-- New permissions (D90). `mail.report` goes where `mail.read` is held (every role above waiting,
-- so a member can report what they were sent). `mail.moderate` reads and closes REPORTS only; it
-- opens no mailbox. `sysop` always holds every permission (a store test fails without its row).
INSERT INTO role_permission (role, permission) VALUES
    ('member',    'mail.report'),
    ('trusted',   'mail.report'),
    ('moderator', 'mail.report'),
    ('cosysop',   'mail.report'),
    ('sysop',     'mail.report'),
    ('moderator', 'mail.moderate'),
    ('cosysop',   'mail.moderate'),
    ('sysop',     'mail.moderate');
