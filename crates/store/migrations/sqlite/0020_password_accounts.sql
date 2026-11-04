-- FIX-M3-10: accounts without an SSH key (D94, D95). Forward-only: once merged this file is
-- never edited (see src/sqlite/tests.rs). Numbered 0020 after M3-I's mail (0019).
--
-- A person who reaches the board over telnet has no key to bind, so the account they make
-- (redeem, `open`, or an approved request or application) is bound to a password instead: a row
-- in `account_password` (0013) and no row in `account_keys`. Nothing about `accounts` changes:
-- an account has always been allowed to have no key rows. The one table that must learn about
-- passwords is the sysop queue, because a request waits days for a decision and the password
-- the person chose has to survive until then. It is kept only as the argon2id hash, in the same
-- PHC format as `account_password`, never the password; the row is deleted when the request is
-- approved (the hash moves to `account_password`), denied or expired. NULL means a request
-- made with an SSH key, which is every row that exists today.
ALTER TABLE invite_request
    ADD COLUMN password_hash TEXT
    CHECK (password_hash IS NULL
           OR (length(password_hash) BETWEEN 1 AND 255 AND substr(password_hash, 1, 10) = '$argon2id$'));
