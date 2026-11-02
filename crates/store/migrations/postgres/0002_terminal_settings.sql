-- M1-G: the Postgres counterpart of the SQLite 0002 migration (M1-E terminal settings).
-- Forward-only: 0001 is untouched, and once merged this file is never edited.

-- One row per account, written the first time the person changes a setting or answers the
-- glyph test. No row means "defaults": follow the PTY, color on, pause on, no tier chosen.
CREATE TABLE terminal_settings (
    account_id      BIGINT  NOT NULL PRIMARY KEY REFERENCES accounts (id) ON DELETE CASCADE,
    -- NULL = follow the PTY size. The valid ranges are enforced in core, not here, so widening
    -- them never needs a migration; the CHECKs only keep nonsense out.
    width_override  INTEGER CHECK (width_override  IS NULL OR width_override  > 0),
    height_override INTEGER CHECK (height_override IS NULL OR height_override > 0),
    color           BOOLEAN NOT NULL,
    pause           BOOLEAN NOT NULL,
    -- NULL = the glyph test has not been answered. Stable text, see core's tier_to_stored.
    icon_tier       TEXT    CHECK (icon_tier IS NULL OR icon_tier IN ('nerd_font', 'unicode', 'ascii')),
    updated_at      BIGINT  NOT NULL DEFAULT (extract(epoch FROM now())::bigint)
);

-- When each account last signed in, and the login before that (what the login screen shows).
CREATE TABLE account_logins (
    account_id        BIGINT NOT NULL PRIMARY KEY REFERENCES accounts (id) ON DELETE CASCADE,
    last_login_at     BIGINT NOT NULL,
    previous_login_at BIGINT
);
