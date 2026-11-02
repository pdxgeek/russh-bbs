-- M2-C: the audit log (D58, docs/audit.md). Append-only, enforced here and not only by the
-- absence of Rust methods. Forward-only: once merged this file is never edited (see
-- src/sqlite/tests.rs). No secrets: ids, closed action names, public fingerprints and a short
-- sanitized note only.
--
-- There is no foreign key on the account or resource columns: a row must outlive the thing it
-- describes, and a cascading delete would be a delete of audit history.

CREATE TABLE audit_log (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    at            INTEGER NOT NULL,
    actor_kind    TEXT    NOT NULL CHECK (actor_kind IN ('system', 'account')),
    actor_id      INTEGER,
    action        TEXT    NOT NULL CHECK (length(action) BETWEEN 1 AND 64),
    target_kind   TEXT    NOT NULL DEFAULT '' CHECK (length(target_kind) <= 32),
    target_ref    TEXT    NOT NULL DEFAULT '' CHECK (length(target_ref) <= 128),
    resource_kind TEXT    NOT NULL DEFAULT '' CHECK (length(resource_kind) <= 32),
    resource_id   INTEGER NOT NULL DEFAULT 0,
    detail        TEXT    NOT NULL DEFAULT '' CHECK (length(detail) <= 500),
    CHECK ((actor_kind = 'account') = (actor_id IS NOT NULL)),
    CHECK ((resource_kind = '' AND resource_id = 0) OR (resource_kind <> '' AND resource_id > 0))
) STRICT;
CREATE INDEX audit_log_by_actor  ON audit_log (actor_kind, actor_id, id);
CREATE INDEX audit_log_by_action ON audit_log (action, id);
CREATE INDEX audit_log_by_time   ON audit_log (at);

CREATE TRIGGER audit_log_no_update BEFORE UPDATE ON audit_log
BEGIN
    SELECT RAISE(ABORT, 'audit_log is append-only');
END;

CREATE TRIGGER audit_log_no_delete BEFORE DELETE ON audit_log
BEGIN
    SELECT RAISE(ABORT, 'audit_log is append-only');
END;

-- `INSERT OR REPLACE` with an existing id would delete the old row without firing the delete
-- trigger (recursive triggers are off by default). Refuse any insert that reuses an id.
CREATE TRIGGER audit_log_no_replace BEFORE INSERT ON audit_log
WHEN NEW.id IS NOT NULL AND EXISTS (SELECT 1 FROM audit_log WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'audit_log is append-only');
END;
