-- FIX-M3-12: a request can carry a password AND a real key (D94 (6), D96). Forward-only: once
-- merged this file is never edited (see src/postgres/tests.rs).
--
-- Since 0020, `password_hash IS NOT NULL` meant "made over telnet, no key". Now an ssh visitor
-- also chooses a password, so a request may hold a real key fingerprint and a hash. `key_bound`
-- says whether `fingerprint` is a key to bind on approval (true) or only the stand-in identity
-- of a keyless request (false). Every row that exists today with a hash is keyless; every other
-- row is a key request.
ALTER TABLE invite_request ADD COLUMN key_bound BOOLEAN NOT NULL DEFAULT TRUE;
UPDATE invite_request SET key_bound = FALSE WHERE password_hash IS NOT NULL;
