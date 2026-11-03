-- M3-C (Postgres counterpart of the SQLite 0011 migration, D24): the event scheduler's
-- persisted last runs and the credits ledger (D79). Forward-only: once merged this file is
-- never edited (see src/postgres/tests.rs).
--
-- `scheduler_job` keeps one row per named job: when it last started and how it ended (a closed
-- name, never a message). `credit_ledger` is append-only like `audit_log` (D59): a row belongs
-- to exactly one account, so nothing here can express a transfer, and there is no balance
-- column; the balance is the sum of the rows. No foreign key on `account_id`: history outlives
-- the account, and a cascading delete would be a delete of ledger history.

CREATE TABLE scheduler_job (
    name         TEXT   PRIMARY KEY CHECK (length(name) BETWEEN 1 AND 32),
    last_run_at  BIGINT NOT NULL,
    last_outcome TEXT   NOT NULL CHECK (last_outcome IN ('ok', 'failed', 'timed-out', 'panicked'))
);

CREATE TABLE credit_ledger (
    id         BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    account_id BIGINT NOT NULL,
    at         BIGINT NOT NULL,
    delta      BIGINT NOT NULL CHECK (delta <> 0 AND delta BETWEEN -1000000000 AND 1000000000),
    reason     TEXT   NOT NULL CHECK (length(reason) BETWEEN 1 AND 32)
);
CREATE INDEX credit_ledger_by_account ON credit_ledger (account_id, id);

CREATE FUNCTION credit_ledger_refuse() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'credit_ledger is append-only' USING ERRCODE = 'restrict_violation';
END;
$$;

-- Row triggers cover UPDATE and DELETE (and so INSERT .. ON CONFLICT DO UPDATE); the
-- statement trigger covers TRUNCATE, which does not fire row triggers.
CREATE TRIGGER credit_ledger_no_update BEFORE UPDATE ON credit_ledger
    FOR EACH ROW EXECUTE FUNCTION credit_ledger_refuse();
CREATE TRIGGER credit_ledger_no_delete BEFORE DELETE ON credit_ledger
    FOR EACH ROW EXECUTE FUNCTION credit_ledger_refuse();
CREATE TRIGGER credit_ledger_no_truncate BEFORE TRUNCATE ON credit_ledger
    FOR EACH STATEMENT EXECUTE FUNCTION credit_ledger_refuse();
