-- M2-C (Postgres counterpart of the SQLite 0004 migration, D24): the audit log (D58,
-- docs/audit.md). Append-only, enforced here and not only by the absence of Rust methods.
-- Forward-only: once merged this file is never edited (see src/postgres/tests.rs). No secrets:
-- ids, closed action names, public fingerprints and a short sanitized note only.
--
-- There is no foreign key on the account or resource columns: a row must outlive the thing it
-- describes, and a cascading delete would be a delete of audit history.

CREATE TABLE audit_log (
    id            BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    at            BIGINT NOT NULL,
    actor_kind    TEXT   NOT NULL CHECK (actor_kind IN ('system', 'account')),
    actor_id      BIGINT,
    action        TEXT   NOT NULL CHECK (length(action) BETWEEN 1 AND 64),
    target_kind   TEXT   NOT NULL DEFAULT '' CHECK (length(target_kind) <= 32),
    target_ref    TEXT   NOT NULL DEFAULT '' CHECK (length(target_ref) <= 128),
    resource_kind TEXT   NOT NULL DEFAULT '' CHECK (length(resource_kind) <= 32),
    resource_id   BIGINT NOT NULL DEFAULT 0,
    detail        TEXT   NOT NULL DEFAULT '' CHECK (length(detail) <= 500),
    CHECK ((actor_kind = 'account') = (actor_id IS NOT NULL)),
    CHECK ((resource_kind = '' AND resource_id = 0) OR (resource_kind <> '' AND resource_id > 0))
);
CREATE INDEX audit_log_by_actor  ON audit_log (actor_kind, actor_id, id);
CREATE INDEX audit_log_by_action ON audit_log (action, id);
CREATE INDEX audit_log_by_time   ON audit_log (at);

CREATE FUNCTION audit_log_refuse() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'audit_log is append-only' USING ERRCODE = 'restrict_violation';
END;
$$;

-- Row triggers cover UPDATE and DELETE (and so INSERT .. ON CONFLICT DO UPDATE); the
-- statement trigger covers TRUNCATE, which does not fire row triggers.
CREATE TRIGGER audit_log_no_update BEFORE UPDATE ON audit_log
    FOR EACH ROW EXECUTE FUNCTION audit_log_refuse();
CREATE TRIGGER audit_log_no_delete BEFORE DELETE ON audit_log
    FOR EACH ROW EXECUTE FUNCTION audit_log_refuse();
CREATE TRIGGER audit_log_no_truncate BEFORE TRUNCATE ON audit_log
    FOR EACH STATEMENT EXECUTE FUNCTION audit_log_refuse();
