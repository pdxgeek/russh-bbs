-- M4-F2: file areas, files, tags and download lists (D-TBD). Forward-only: once merged this file
-- is never edited (see src/sqlite/tests.rs).
--
-- Bytes are not here. A file row names a blob (`blob_key`, `<namespace>/<id>`, minted by core)
-- in the blob store (D30, D106); the database holds what the board knows about it. A file is
-- `quarantine` from upload until staff approve it (or its area auto-approves), and the blob sits
-- in the `quarantine` namespace until then; `approved` files live in `files`. A `rejected` row is
-- kept for the record with its blob deleted.

CREATE TABLE file_area (
    id           INTEGER PRIMARY KEY,
    name         TEXT    NOT NULL CHECK (length(name) BETWEEN 1 AND 160),
    name_key     TEXT    NOT NULL UNIQUE,
    description  TEXT    NOT NULL DEFAULT '' CHECK (length(description) <= 640),
    restricted   INTEGER NOT NULL DEFAULT 0 CHECK (restricted IN (0, 1)),
    auto_approve INTEGER NOT NULL DEFAULT 0 CHECK (auto_approve IN (0, 1)),
    created_by   INTEGER REFERENCES accounts (id) ON DELETE SET NULL,
    created_at   INTEGER NOT NULL
) STRICT;

CREATE TABLE file (
    id            INTEGER PRIMARY KEY,
    area_id       INTEGER NOT NULL REFERENCES file_area (id) ON DELETE CASCADE,
    uploader_id   INTEGER REFERENCES accounts (id) ON DELETE SET NULL,
    -- The name members see and transfer under: through the shared sanitizer.
    name          TEXT    NOT NULL CHECK (length(name) BETWEEN 1 AND 480),
    -- The name as the client sent it. Data only: never drawn, never used as a path.
    original_name TEXT    NOT NULL CHECK (length(original_name) <= 1020),
    description   TEXT    NOT NULL DEFAULT '' CHECK (length(description) <= 1920),
    size          INTEGER NOT NULL CHECK (size >= 0),
    sha256        TEXT    NOT NULL CHECK (length(sha256) = 64 AND sha256 NOT GLOB '*[^0-9a-f]*'),
    blob_key      TEXT    NOT NULL CHECK (length(blob_key) BETWEEN 3 AND 80),
    state         TEXT    NOT NULL CHECK (state IN ('quarantine', 'approved', 'rejected')),
    scan          TEXT    NOT NULL CHECK (scan IN ('unscanned', 'clean', 'infected', 'failed')),
    scan_note     TEXT    NOT NULL DEFAULT '' CHECK (length(scan_note) <= 480),
    created_at    INTEGER NOT NULL,
    decided_at    INTEGER,
    decided_by    INTEGER REFERENCES accounts (id) ON DELETE SET NULL,
    downloads     INTEGER NOT NULL DEFAULT 0 CHECK (downloads >= 0)
) STRICT;
CREATE INDEX file_by_area_state ON file (area_id, state, id);
CREATE INDEX file_by_uploader ON file (uploader_id, state, id);

CREATE TABLE file_tag (
    file_id INTEGER NOT NULL REFERENCES file (id) ON DELETE CASCADE,
    tag     TEXT    NOT NULL CHECK (length(tag) BETWEEN 1 AND 24 AND tag NOT GLOB '*[^a-z0-9_-]*'),
    PRIMARY KEY (file_id, tag)
) STRICT;
CREATE INDEX file_tag_by_tag ON file_tag (tag);

-- A member's batch download list (`/downloads` over SFTP). It goes with the account.
CREATE TABLE download_list (
    account_id INTEGER NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    file_id    INTEGER NOT NULL REFERENCES file (id) ON DELETE CASCADE,
    added_at   INTEGER NOT NULL,
    PRIMARY KEY (account_id, file_id)
) STRICT;
