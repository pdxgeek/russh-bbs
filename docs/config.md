# Configuration

Implemented in M1-H (`russh-bbs-server::config`). One TOML file, `RUSSH_BBS_*` environment variables and command-line flags all set the same typed struct, validated at boot. [`config.example.toml`](../config.example.toml) lists every key at its default; a test checks that it parses, validates and equals the built-in defaults.

## Precedence

Lowest to highest: **built-in defaults < config file < environment < command-line flags.** A key set in a higher layer replaces the same key from a lower one, key by key.

## Where the file is

1. `--config <path>`, else `RUSSH_BBS_CONFIG`. A file named this way that does not exist is an **error**.
2. Otherwise the first of these that exists is read:
   - `$XDG_CONFIG_HOME/russh-bbs/config.toml` (only if `XDG_CONFIG_HOME` is absolute), else `$HOME/.config/russh-bbs/config.toml`
   - `/data/config.toml` (the container's volume, D25)
3. If none exists, the server runs on defaults. That is not an error.

Relative paths inside the file are relative to the server's working directory. Use absolute paths.

## Kubernetes service links

A strict rule applies to the environment: a `RUSSH_BBS_*` variable that no setting reads stops boot, naming the variable, so a typo in a real setting is an error (D56). Kubernetes injects service-link variables for every service in the namespace, and a service called `russh-bbs` yields `RUSSH_BBS_PORT`, `RUSSH_BBS_PORT_<port>_<PROTO>` (and `_PROTO`, `_PORT`, `_ADDR` suffixes), `RUSSH_BBS_SERVICE_HOST` and `RUSSH_BBS_SERVICE_PORT[_<NAME>]`. These exact names and prefixes are ignored, each with a boot warning naming the variable. Nothing else is ignored. Set `enableServiceLinks: false` on the pod to stop the injection altogether.

## Validation at boot

- **Unknown keys are rejected.** In the file the message names the key and the file (`unknown key `ssh.listn` in config file …`); an unknown `RUSSH_BBS_*` environment variable is rejected too, so a typo cannot silently do nothing.
- **Bad values** (wrong type, out of range, bad path, inconsistent with another key) name the key and where the value came from: the file, the environment variable or the flag.
- **Something that is parsed but not built is a boot error, not ignored.** `datasource.iam_auth = true` fails with "IAM token authentication is not built yet" (D31, D44). The server never falls back to SQLite from Postgres, or to another kind of authentication, on its own.
- **Postgres failures stop the boot.** A bad URL, an unreachable server, a TLS check that fails, a server that does not offer TLS, or a refused login is a boot error that names the cause and never contains the password.
- **Settings that parse but do nothing yet** log a warning when turned on (S3 blobs), so the sysop is not misled. Settings that lower the board's security log a warning at start: password login (M3-D) says it is on, and telnet (M3-H) says the board is in low-security mode.
- **Secrets are redacted.** `datasource.url` may contain a password. It has no command-line flag (argv is visible in `ps`), it is masked in `Debug`, `Display` and logs (`postgres://user:***@host/db`), and no error message quotes it or the line it was on. If a config file sets it and is readable by group or others the server warns; use `chmod 600`. Object-store credentials are never read from this file: they come from the environment (D30).

## Environment variable names

A key's environment variable is listed below. Names that existed before this file (`RUSSH_BBS_LISTEN`, `RUSSH_BBS_HOST_KEY`, `RUSSH_BBS_DB`, the timeout, connection and rate-limit names) are kept. All others are `RUSSH_BBS_<SECTION>_<KEY>`. Booleans are `true` or `false`; integers are whole numbers.

## Keys

"Used by" says what reads the key today. **Parsed, not yet used** means the value is validated and kept but no feature reads it.

### Top level

| Key | Type | Default | Environment | Flag | Used by |
|---|---|---|---|---|---|
| `registration_mode` | `"invite"`, `"apply"`, `"open"` | `"invite"` | `RUSSH_BBS_REGISTRATION_MODE` | none | Waiting room (M1-C), through the stored setting; see below |

**How `registration_mode` meets the stored setting (D38).** The board keeps the mode in its database (`settings`), where M1-B reads it and M2's sysop console will change it. If the file, the environment or a flag sets `registration_mode`, the server writes that value into the store at every start and it wins. If none of them sets it, the stored value is left alone (a fresh database reads as `invite`). So the default never overwrites a mode the sysop chose in the console, and a sysop who wants the file to be the authority sets it there.

### `[ssh]`

| Key | Type | Default | Environment | Flag | Used by |
|---|---|---|---|---|---|
| `ssh.listen` | address `host:port`, port 1–65535 | `0.0.0.0:2222` | `RUSSH_BBS_LISTEN` | `--listen` | SSH listener (M1-A) |
| `ssh.host_key` | path | `$XDG_DATA_HOME/russh-bbs/host_ed25519_key`, else `~/.local/share/russh-bbs/host_ed25519_key`; in the container (no usable `HOME`, `/data` exists) `/data/host_ed25519_key` (D46) | `RUSSH_BBS_HOST_KEY` | `--host-key` | Host key (M1-A, D20). With no default location and no value the server refuses to start |
| `ssh.auth_timeout_secs` | integer 1–3600 | `30` | `RUSSH_BBS_AUTH_TIMEOUT_SECS` | `--auth-timeout-secs` | Accept loop (M1-A, D22) |
| `ssh.idle_timeout_secs` | integer 1–86400 | `600` | `RUSSH_BBS_IDLE_TIMEOUT_SECS` | `--idle-timeout-secs` | SSH sessions (M1-A) |
| `ssh.keepalive_interval_secs` | integer 1–3600 | `30` | `RUSSH_BBS_SSH_KEEPALIVE_INTERVAL_SECS` | none | SSH sessions (M1-A) |
| `ssh.max_connections` | integer 1–1000000 | `64` | `RUSSH_BBS_MAX_CONNECTIONS` | `--max-connections` | Accept loop (M1-A, D53). This times the render cap is the memory ceiling for render buffers: each session may hold two buffers of `render_cols` x `render_rows` cells at 48 bytes a cell (about 9.6 MB at the default cap, 0.2 MB for a member at 80x24). The server logs the ceiling at start (`render_memory_ceiling_mib`) |
| `ssh.render_cols` | integer 80–500 | `500` | `RUSSH_BBS_SSH_RENDER_COLS` | none | SSH sessions (D53). Widest area one session draws into; a wider window is centered on it and the rest stays blank. Lower it on a small host |
| `ssh.render_rows` | integer 24–200 | `200` | `RUSSH_BBS_SSH_RENDER_ROWS` | none | SSH sessions (D53). Tallest area one session draws into; see `render_cols` |
| `ssh.max_auth_attempts` | integer 1–100 | `6` | `RUSSH_BBS_SSH_MAX_AUTH_ATTEMPTS` | none | SSH handler (M1-A) |
| `ssh.auth_rejection_millis` | integer 0–60000 | `1000` | `RUSSH_BBS_SSH_AUTH_REJECTION_MILLIS` | none | SSH handler (M1-A) |
| `ssh.redraw_interval_millis` | integer 10–5000 | `50` | `RUSSH_BBS_SSH_REDRAW_INTERVAL_MILLIS` | none | SSH sessions (FIX-M1-1, D49). One session redraws at most `1000 / value` times a second (20 by default). Window changes are folded into the latest size and keys into one batch per redraw |
| `ssh.requests_per_second` | integer 10–100000 | `200` | `RUSSH_BBS_SSH_REQUESTS_PER_SECOND` | none | SSH sessions (FIX-M1-1, D49). Data packets and window changes per second one session may send, with a two-second burst. Excess data packets are dropped; an excess window change only updates the size to apply |
| `ssh.input_bytes_per_second` | integer 64–10000000 | `8192` | `RUSSH_BBS_SSH_INPUT_BYTES_PER_SECOND` | none | SSH sessions (FIX-M1-1, D49). Input bytes per second one session may send, with a two-second burst (16 KiB by default, enough for a long paste). Excess bytes are dropped |
| `ssh.flood_strikes` | integer 1–1000000 | `1000` | `RUSSH_BBS_SSH_FLOOD_STRIKES` | none | SSH sessions (FIX-M1-1, D49). Over-budget events (each excess request; each started KiB of excess input) before the session is disconnected. Forgotten after 10 quiet seconds |
| `ssh.write_stall_secs` | integer 1–3600 | `10` | `RUSSH_BBS_SSH_WRITE_STALL_SECS` | none | SSH sessions (FIX-M1-1, D49). How long a peer may leave output waiting (its window or socket full) before the connection is dropped instead of queueing without bound |
| `ssh.allow_password_login` | boolean | `true` | `RUSSH_BBS_SSH_ALLOW_PASSWORD_LOGIN` | none | Password login over SSH (M3-D, D81; **on by default since D94 (10), D96**: ssh is the transport and the account is handle plus password). On: the SSH `password` method is offered (not keyboard-interactive) and every account has a password (it is asked for when joining), so `ssh <handle>@host` signs in with it unless a key bound to that handle does first. Off: a password sign-in over ssh is refused (the `password` method still serves key-less visitors under `new` and `claim`), a warning is logged at start, and a member who joined with a password can sign in over ssh only by key. Wrong passwords back off gently per handle (4 free, then 1, 2, 4, 8, 16 and 30 s; one right password clears it) and more firmly per network (3 free, then 5, 10, 20 s and on up to 5 min), a key is never held by either, and every failure looks the same (D105). These numbers are `ssh::Limits` defaults, not config keys. A refused password keeps `password` on offer, so one connection may try up to `ssh.max_auth_attempts` times (all methods count together) before the server hangs up; the per-handle and per-network limits apply to every try. Setting a password is possible whether this is on or off. |

### `[datasource]`

| Key | Type | Default | Environment | Flag | Used by |
|---|---|---|---|---|---|
| `datasource.kind` | `"sqlite"`, `"postgres"` | `"sqlite"` | `RUSSH_BBS_DATASOURCE_KIND` | none | Store selection (M1-G). Both kinds are built; there is no fallback between them |
| `datasource.path` | path (only with `sqlite`) | `$XDG_DATA_HOME/russh-bbs/russh-bbs.db`, else `~/.local/share/russh-bbs/russh-bbs.db`; in the container `/data/russh-bbs.db` (D46) | `RUSSH_BBS_DB` | `--db` | SQLite store (M1-B) |
| `datasource.url` | `postgres://` or `postgresql://` URL, required and only with `postgres` | unset | `RUSSH_BBS_DATASOURCE_URL` | none (secret) | Postgres store (M1-G). Put the host name, not a unix socket path. Do not put `sslmode` in the URL: it must equal `tls_mode` or boot is refused |
| `datasource.tls_mode` | `"require"`, `"verify-ca"`, `"verify-full"` (only with `postgres`) | `"require"` | `RUSSH_BBS_DATASOURCE_TLS_MODE` | none | Postgres store (M1-G, D31). There is no value that turns TLS off. **`verify-ca` is currently as strict as `verify-full`: it also fails on a host name that is not in the certificate** (sqlx 0.8.6 means to ignore a name mismatch for `verify-ca`, but it only matches rustls's `NotValidForName` error and rustls 0.23.46 returns `NotValidForNameContext`, so the relaxation never applies; checked in the sqlx and rustls sources and live in QA M1). Do not rely on the difference; a dependency upgrade may restore it. **`require` encrypts but does not check the certificate unless `tls_root_cert` is set**; use `verify-full` (with `tls_root_cert` for a private CA or the RDS bundle) to defend against an active attacker |
| `datasource.tls_root_cert` | path to a PEM file that exists (only with `postgres`) | unset | `RUSSH_BBS_DATASOURCE_TLS_ROOT_CERT` | none | Postgres store (M1-G). The CA that signed the server certificate; a self-signed server certificate that is not itself a CA is rejected. Without it the platform CA store is used; **the published container image is `scratch` and has none, so with `verify-ca`/`verify-full` in the container set `tls_root_cert` to a file on the `/data` volume** (`require` works without) |
| `datasource.iam_auth` | boolean (only with `postgres`) | `false` | `RUSSH_BBS_DATASOURCE_IAM_AUTH` | none | Parsed, **refused at boot when `true`** (not built; D31, D44) |

Setting a key that belongs to the other backend (for example `datasource.url` with `kind = "sqlite"`) is an error, not something silently ignored. MySQL is not a choice.

### `[blobs]` (D30)

All parsed, not yet used: no blob store exists yet. Object-store credentials come from the environment, never from this file.

| Key | Type | Default | Environment | Flag | Used by |
|---|---|---|---|---|---|
| `blobs.backend` | `"local"`, `"s3"` | `"local"` | `RUSSH_BBS_BLOBS_BACKEND` | none | Parsed, not yet used; warns when `s3` |
| `blobs.dir` | path (only with `local`) | unset (a directory under the default data directory) | `RUSSH_BBS_BLOBS_DIR` | none | Parsed, not yet used |
| `blobs.endpoint` | `http(s)://` URL without credentials (only with `s3`) | unset (AWS) | `RUSSH_BBS_BLOBS_ENDPOINT` | none | Parsed, not yet used |
| `blobs.bucket` | string, required with `s3` | unset | `RUSSH_BBS_BLOBS_BUCKET` | none | Parsed, not yet used |
| `blobs.region` | string (only with `s3`) | unset | `RUSSH_BBS_BLOBS_REGION` | none | Parsed, not yet used |

### `[mail]` (M3-I, D90)

| Key | Type | Default | Environment | Flag | Used by |
|---|---|---|---|---|---|
| `mail.key_file` | path, not the same file as `ssh.host_key` | `mail.key` in the data directory, beside the host key (`$XDG_DATA_HOME/russh-bbs/mail.key`, else `~/.local/share/russh-bbs/mail.key`; in the container `/data/mail.key`) | `RUSSH_BBS_MAIL_KEY_FILE` | none (a path, not a secret, but no flag is needed) | Internal mail (M3-I): the key that seals stored mail |

The mail key is 54 bytes in a small checked format, created with mode 0600 **only on a fresh install** (the database holds no mail). It is never in the database and never logged (the log shows its path and key id only). If the file is missing while the database holds mail, or it is damaged, the wrong size, not a mail key file, or readable by group or others, the server **refuses to start** and says so; it never makes a replacement, because a new key could not open the mail already stored. **Back this file up separately from the database (M5 backups will do it for you): without it every stored message is lost for good, and with it a copy of the database is readable.** See [deploy.md](deploy.md#backups).

### `[telnet]` (D27)

Built in M3-H. Turning telnet on is "low-security mode": an unencrypted listener on its own port, with a loud warning in the log and a `LOW-SECURITY` marker on staff screens. **When `telnet.enabled` is `false` (the default) no port is bound.** Telnet follows the `[ssh]` limits and timeouts (`max_connections` and the rest) and the `[waiting_room]` per-network connection rate, with its own counts; see D85.

What a telnet session is (D94, D95): a password login (a second opt-in, below) or, by typing `new` at the login prompt, the same waiting room as SSH in every registration mode, where redeeming, applying and open registration ask for a password and make a **password account** (no SSH key). After login it is the same member screens as SSH, including mail, invites and the password page, with the fixed `UNENCRYPTED` marker on the last row of every screen. It is never a founder claim, SFTP or a staff session: roles of rank 40 and up (`cosysop`, `sysop`) are dropped, so a sysop who logs in over telnet is an ordinary member. Only ASCII passwords can be typed.

| Key | Type | Default | Environment | Flag | Used by |
|---|---|---|---|---|---|
| `telnet.enabled` | boolean | `false` | `RUSSH_BBS_TELNET_ENABLED` | none | The telnet listener (M3-H); off binds no port; on warns at start |
| `telnet.allow_login` | boolean, needs `telnet.enabled` | `false` | `RUSSH_BBS_TELNET_ALLOW_LOGIN` | none | Member password login and the waiting room over telnet; off shows a two-line refusal and closes |
| `telnet.listen` | address, port 1–65535, not the SSH port when enabled | `0.0.0.0:2323` | `RUSSH_BBS_TELNET_LISTEN` | none | The telnet listener's address |

### `[feeds]` (D89)

Built in M3-G. **RSS in:** the sysop adds a feed on the forums screen (`f` on the board list, needs `feeds.manage`); the feed becomes a read-only board and a scheduled job reads it. **With `feeds.enabled = false` (the default) the server makes no outbound request for feeds**; feeds can still be added and removed, they are just not read. The fetcher resolves the name, refuses every private, loopback, link-local (cloud metadata), carrier-grade NAT, multicast, unspecified, documentation and reserved address (and IPv6 forms that embed one), and connects to the address it checked. Redirects are re-checked hop by hop (at most 3, never from https to http); the whole fetch is capped at 20 seconds, 5 seconds to connect, 1 MiB of body and 100 items. https is required unless the sysop flags that one feed as allowing plain http. **It never uses a proxy:** `HTTPS_PROXY`, `HTTP_PROXY` and `ALL_PROXY` are ignored (a proxy resolves the name itself, so the guard could not check the address it connects to) and the server logs a warning at start when one is set; a board that can only reach the internet through a proxy cannot read feeds.

**RSS out:** boards the sysop marked public (`p` on a board's thread list) are served as RSS 2.0 at `<rss_out_base_url>/rss/<board id>` by a small HTTP listener. **When `feeds.rss_out_enabled` is `false` (the default) no port is bound.** The listener speaks plain HTTP and serves GET only; put a TLS proxy in front of it or keep it on a private address. It serves exactly what an anonymous reader may read: a board that is absent, not public or restricted gets the same 404.

| Key | Type | Default | Environment | Flag | Used by |
|---|---|---|---|---|---|
| `feeds.enabled` | boolean | `false` | `RUSSH_BBS_FEEDS_ENABLED` | none | Registers the feed-reading job with the scheduler (M3-G); off makes no outbound request |
| `feeds.rss_out_enabled` | boolean, needs `feeds.rss_out_base_url` | `false` | `RUSSH_BBS_FEEDS_RSS_OUT_ENABLED` | none | The RSS listener (M3-G); off binds no port |
| `feeds.rss_out_listen` | address, port 1–65535, not the SSH or telnet port | `127.0.0.1:8081` | `RUSSH_BBS_FEEDS_RSS_OUT_LISTEN` | none | The RSS listener's address |
| `feeds.ca_file` | path to a PEM file of CA certificates that exists | unset (the platform store) | `RUSSH_BBS_FEEDS_CA_FILE` | none | The CAs https feeds are verified against (D92). **When set it replaces the platform store for feed fetching only**: the Postgres connection (`datasource.tls_root_cert`) and everything else are unaffected, and certificates outside the file are not trusted. The file must exist at boot and hold at least one PEM certificate; a missing, empty or malformed file is a boot error with `feeds.enabled`, never a silent fall back to the platform store. Unset, the fetcher uses the platform store: in the container image that is the bundled Mozilla CA list (`SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt`). Needed only for a feed served with a private CA, or to pin a smaller set |
| `feeds.rss_out_base_url` | http or https address, no trailing slash, no credentials or query | none | `RUSSH_BBS_FEEDS_RSS_OUT_BASE_URL` | none | The public address readers reach the listener at, used in the documents' links; required with `feeds.rss_out_enabled` |

### `[waiting_room]` (D37)

| Key | Type | Default | Environment | Flag | Used by |
|---|---|---|---|---|---|
| `waiting_room.ip_burst` | integer 1–1000000 | `10` | `RUSSH_BBS_IP_BURST` | `--ip-burst` | Per-network connection limit (M1-C) |
| `waiting_room.ip_interval_secs` | integer 1–86400 | `2` | `RUSSH_BBS_IP_INTERVAL_SECS` | `--ip-interval-secs` | Per-network connection limit (M1-C) |
| `waiting_room.key_burst` | integer 1–1000000 | `5` | `RUSSH_BBS_KEY_BURST` | `--key-burst` | Per-key authentication limit (M1-C) |
| `waiting_room.key_interval_secs` | integer 1–86400 | `10` | `RUSSH_BBS_KEY_INTERVAL_SECS` | `--key-interval-secs` | Per-key authentication limit (M1-C) |
| `waiting_room.max_form_submissions` | integer 1–1000 | `5` | `RUSSH_BBS_MAX_FORM_SUBMISSIONS` | `--max-form-submissions` | Waiting-room forms (M1-C) |
| `waiting_room.max_tracked_sources` | integer 1–1000000 | `10000` | `RUSSH_BBS_WAITING_ROOM_MAX_TRACKED_SOURCES` | none | Rate-limiter memory bound (M1-C) |

The waiting-room banner and rules text are stored settings (D37), not config keys. They are limited so the menu always fits an 80x24 terminal (D64): the banner to 300 characters and 5 lines, the rules to 900 characters and 10 lines. Text stored past that is cut when shown and ends with an ellipsis; the screen that sets them (M2) refuses longer text with an error that names the limit.

The invite-request queue's cap is a stored setting too (M2-E, D72): `queue_max_pending`, a whole number from 1 to 10,000, **default 200**. A stored value that is not such a number is read as the default (it is logged), never as "no cap". Until the M5 settings screen exists, set it in the `settings` table (`INSERT INTO settings (key, value) VALUES ('queue_max_pending', '50') ON CONFLICT (key) DO UPDATE SET value = excluded.value`); it is read on every submission, so it applies at once. The other numbers of the queue (one request per key, the rate limits, the 14-day expiry, the 24-hour cooldown after a denial) are constants in `core::queue`; see [queue.md](queue.md).

`open` registration's daily cap is a stored setting too (M2-G, D73), next to `registration_mode`: `open_daily_cap`, the most accounts `open` mode may create in one **UTC day**: a whole number from 1 to 100,000, or `0` for no cap. **Default 25** when unset. A stored value that is blank, not a number or out of range is read as the default (it is logged), never as "no cap"; `0` is the only way to turn the cap off. When the day's places are used the board shows the `apply` screens with a notice until 00:00 UTC. It is read per connection, so a change needs no restart. Until the M5 settings screen exists, set it in the `settings` table (`INSERT INTO settings (key, value) VALUES ('open_daily_cap', '50') ON CONFLICT (key) DO UPDATE SET value = excluded.value`). `registration_mode` (D38) is read per connection in the same way. The `open` account gate (per key 2 then 1 an hour, per network 5 then 1 every 10 minutes, D74) is a constant in `core::registration`, not a key.

The per-address connection limit (`waiting_room.ip_burst`, `waiting_room.ip_interval_secs`) also applies to the founder claim; see "If the claim connection is dropped" in [deploy.md](deploy.md) (D65).

## Resource limits (M2-H)

Limits are **not config keys**: they are board data, edited by a member holding `limits.edit` (a sysop screen arrives in M5; the core service is `core::limits`), stored in `role_limit` and `member_limit`, and effective without a restart (an open session notices within 30 seconds). Only the seeded **defaults** are described here; the rules, kinds and enforcement status are in [access.md](access.md#resource-limits). A test keeps this table equal to the migration's rows.

| Role | Minutes/day | Calls/day | Sessions | Posts/hour | Upload/day | Upload/file | Mail/day | Invites/month |
|---|---|---|---|---|---|---|---|---|
| `member` | 60 | 5 | 2 | 20 | 10 MiB | 5 MiB | 20 | 2 |
| `trusted` | 120 | 10 | 3 | 40 | 50 MiB | 10 MiB | 50 | 5 |
| `moderator` | 240 | 20 | 4 | 100 | 100 MiB | 25 MiB | 100 | 10 |
| `cosysop` | unlimited | unlimited | 8 | 200 | 1024 MiB | 100 MiB | 500 | 50 |
| `sysop` | unlimited | unlimited | unlimited | unlimited | unlimited | unlimited | unlimited | unlimited |

Usage counters (`limit_usage`) are per account per UTC day and survive a restart. The concurrent-session count is in memory, per process.

## Other flags and variables

| Name | Meaning |
|---|---|
| `--config <path>` | The config file (see above) |
| `RUSSH_BBS_CONFIG` | Same as `--config`; the flag wins |
| `RUST_LOG` | Log filter (default `info`); not a `RUSSH_BBS_*` setting |

## Implemented / Planned / Verified

- **Implemented and tested (M1-H):** all of the above; unit tests cover each layer, precedence, unknown keys, bad values with their source, the IAM refusal, redaction in `Debug`, `Display` and captured logs, default file search, the example file equalling the defaults, and this document naming every key, variable and flag.
- **Implemented and tested (M1-G):** the `postgres` datasource (D44): config wiring and the `iam_auth` refusal in the config tests, the store itself in the shared suite on Postgres 16 and on a TLS-required Postgres.
- **Implemented and tested (M3-H):** the `[telnet]` section is used. Off binds nothing; on starts the listener, warns, and sets `Limits::low_security`. Config tests cover the defaults, the warning and the limits flag; the transport is covered in `tests/telnet.rs` (see D85 and [features.md](features.md)).
- **Implemented and tested (FIX-M3-5, D92):** `feeds.ca_file` (existence check at boot; the fetcher loads it and fails closed on a missing, empty, malformed or wrong CA). Tests: `feeds::tests::ca_file_replaces_the_platform_store_and_a_wrong_ca_fails_closed`, `a_ca_file_that_is_missing_empty_or_not_pem_is_an_error`, `config::tests::feeds_ca_file_must_name_an_existing_file_and_defaults_to_the_platform_store`. Not verified: a live https feed from the container (this environment reaches the internet only through a proxy the fetcher refuses to use).
- **Implemented and tested (M3-G):** the `[feeds]` section is used (D89). Config tests cover the defaults, the base-URL rule and port clashes; the fetcher and the listener are covered by the feed tests in `crates/server/src/feeds` (see D89 and [features.md](features.md)).
- **Planned:** IAM token authentication, a blob store reading `[blobs]`.
- **Verified end to end:** see the M1-H pull request for the `ssh` session against a file and against environment overrides.
