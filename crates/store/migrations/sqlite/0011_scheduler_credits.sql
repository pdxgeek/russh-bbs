-- M3-C: the event scheduler's persisted last runs and the credits ledger (D79).
-- Forward-only: once merged this file is never edited (see src/sqlite/tests.rs).
--
-- `scheduler_job` keeps one row per named job: when it last started and how it ended (a closed
-- name, never a message). `credit_ledger` is append-only like `audit_log` (D59): a row belongs
-- to exactly one account, so nothing here can express a transfer, and there is no balance
-- column; the balance is the sum of the rows. No foreign key on `account_id`: history outlives
-- the account, and a cascading delete would be a delete of ledger history.

CREATE TABLE scheduler_job (
    name         TEXT    PRIMARY KEY CHECK (length(name) BETWEEN 1 AND 32),
    last_run_at  INTEGER NOT NULL,
    last_outcome TEXT    NOT NULL CHECK (last_outcome IN ('ok', 'failed', 'timed-out', 'panicked'))
) STRICT;

CREATE TABLE credit_ledger (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id INTEGER NOT NULL,
    at         INTEGER NOT NULL,
    delta      INTEGER NOT NULL CHECK (delta <> 0 AND delta BETWEEN -1000000000 AND 1000000000),
    reason     TEXT    NOT NULL CHECK (length(reason) BETWEEN 1 AND 32)
) STRICT;
CREATE INDEX credit_ledger_by_account ON credit_ledger (account_id, id);

CREATE TRIGGER credit_ledger_no_update BEFORE UPDATE ON credit_ledger
BEGIN
    SELECT RAISE(ABORT, 'credit_ledger is append-only');
END;

CREATE TRIGGER credit_ledger_no_delete BEFORE DELETE ON credit_ledger
BEGIN
    SELECT RAISE(ABORT, 'credit_ledger is append-only');
END;

-- `INSERT OR REPLACE` with an existing id would delete the old row without firing the delete
-- trigger (recursive triggers are off by default). Refuse any insert that reuses an id.
CREATE TRIGGER credit_ledger_no_replace BEFORE INSERT ON credit_ledger
WHEN NEW.id IS NOT NULL AND EXISTS (SELECT 1 FROM credit_ledger WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'credit_ledger is append-only');
END;
