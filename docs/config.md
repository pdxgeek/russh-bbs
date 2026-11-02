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

## Validation at boot

- **Unknown keys are rejected.** In the file the message names the key and the file (`unknown key `ssh.listn` in config file …`); an unknown `RUSSH_BBS_*` environment variable is rejected too, so a typo cannot silently do nothing.
- **Bad values** (wrong type, out of range, bad path, inconsistent with another key) name the key and where the value came from: the file, the environment variable or the flag.
- **Something that is parsed but not built is a boot error, not ignored.** `datasource.iam_auth = true` fails with "IAM token authentication is not built yet" (D31, D43). The server never falls back to SQLite from Postgres, or to another kind of authentication, on its own.
- **Postgres failures stop the boot.** A bad URL, an unreachable server, a TLS check that fails, a server that does not offer TLS, or a refused login is a boot error that names the cause and never contains the password.
- **Settings that parse but do nothing yet** log a warning when turned on (password login, telnet, S3 blobs), so the sysop is not misled.
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
| `ssh.host_key` | path | `$XDG_DATA_HOME/russh-bbs/host_ed25519_key`, else `~/.local/share/russh-bbs/host_ed25519_key` | `RUSSH_BBS_HOST_KEY` | `--host-key` | Host key (M1-A, D20). With no default location and no value the server refuses to start |
| `ssh.auth_timeout_secs` | integer 1–3600 | `30` | `RUSSH_BBS_AUTH_TIMEOUT_SECS` | `--auth-timeout-secs` | Accept loop (M1-A, D22) |
| `ssh.idle_timeout_secs` | integer 1–86400 | `600` | `RUSSH_BBS_IDLE_TIMEOUT_SECS` | `--idle-timeout-secs` | SSH sessions (M1-A) |
| `ssh.keepalive_interval_secs` | integer 1–3600 | `30` | `RUSSH_BBS_SSH_KEEPALIVE_INTERVAL_SECS` | none | SSH sessions (M1-A) |
| `ssh.max_connections` | integer 1–1000000 | `256` | `RUSSH_BBS_MAX_CONNECTIONS` | `--max-connections` | Accept loop (M1-A) |
| `ssh.max_auth_attempts` | integer 1–100 | `6` | `RUSSH_BBS_SSH_MAX_AUTH_ATTEMPTS` | none | SSH handler (M1-A) |
| `ssh.auth_rejection_millis` | integer 0–60000 | `1000` | `RUSSH_BBS_SSH_AUTH_REJECTION_MILLIS` | none | SSH handler (M1-A) |
| `ssh.allow_password_login` | boolean | `false` | `RUSSH_BBS_SSH_ALLOW_PASSWORD_LOGIN` | none | Parsed, not yet used (D28; warns when on) |

### `[datasource]`

| Key | Type | Default | Environment | Flag | Used by |
|---|---|---|---|---|---|
| `datasource.kind` | `"sqlite"`, `"postgres"` | `"sqlite"` | `RUSSH_BBS_DATASOURCE_KIND` | none | Store selection (M1-G). Both kinds are built; there is no fallback between them |
| `datasource.path` | path (only with `sqlite`) | `$XDG_DATA_HOME/russh-bbs/russh-bbs.db`, else `~/.local/share/russh-bbs/russh-bbs.db` | `RUSSH_BBS_DB` | `--db` | SQLite store (M1-B) |
| `datasource.url` | `postgres://` or `postgresql://` URL, required and only with `postgres` | unset | `RUSSH_BBS_DATASOURCE_URL` | none (secret) | Postgres store (M1-G). Put the host name, not a unix socket path. Do not put `sslmode` in the URL: it must equal `tls_mode` or boot is refused |
| `datasource.tls_mode` | `"require"`, `"verify-ca"`, `"verify-full"` (only with `postgres`) | `"require"` | `RUSSH_BBS_DATASOURCE_TLS_MODE` | none | Postgres store (M1-G, D31). There is no value that turns TLS off. **`require` encrypts but does not check the certificate unless `tls_root_cert` is set**; use `verify-full` (with `tls_root_cert` for a private CA or the RDS bundle) to defend against an active attacker |
| `datasource.tls_root_cert` | path to a PEM file that exists (only with `postgres`) | unset | `RUSSH_BBS_DATASOURCE_TLS_ROOT_CERT` | none | Postgres store (M1-G). The CA that signed the server certificate; a self-signed server certificate that is not itself a CA is rejected. Without it the platform CA store is used (the container needs `ca-certificates`) |
| `datasource.iam_auth` | boolean (only with `postgres`) | `false` | `RUSSH_BBS_DATASOURCE_IAM_AUTH` | none | Parsed, **refused at boot when `true`** (not built; D31, D43) |

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

### `[telnet]` (D27)

Parsed, not yet used (M3). Turning telnet on is "low-security mode".

| Key | Type | Default | Environment | Flag | Used by |
|---|---|---|---|---|---|
| `telnet.enabled` | boolean | `false` | `RUSSH_BBS_TELNET_ENABLED` | none | Parsed, not yet used; warns when on |
| `telnet.allow_login` | boolean, needs `telnet.enabled` | `false` | `RUSSH_BBS_TELNET_ALLOW_LOGIN` | none | Parsed, not yet used |
| `telnet.listen` | address, port 1–65535, not the SSH port when enabled | `0.0.0.0:2323` | `RUSSH_BBS_TELNET_LISTEN` | none | Parsed, not yet used |

### `[waiting_room]` (D37)

| Key | Type | Default | Environment | Flag | Used by |
|---|---|---|---|---|---|
| `waiting_room.ip_burst` | integer 1–1000000 | `10` | `RUSSH_BBS_IP_BURST` | `--ip-burst` | Per-network connection limit (M1-C) |
| `waiting_room.ip_interval_secs` | integer 1–86400 | `2` | `RUSSH_BBS_IP_INTERVAL_SECS` | `--ip-interval-secs` | Per-network connection limit (M1-C) |
| `waiting_room.key_burst` | integer 1–1000000 | `5` | `RUSSH_BBS_KEY_BURST` | `--key-burst` | Per-key authentication limit (M1-C) |
| `waiting_room.key_interval_secs` | integer 1–86400 | `10` | `RUSSH_BBS_KEY_INTERVAL_SECS` | `--key-interval-secs` | Per-key authentication limit (M1-C) |
| `waiting_room.max_form_submissions` | integer 1–1000 | `5` | `RUSSH_BBS_MAX_FORM_SUBMISSIONS` | `--max-form-submissions` | Waiting-room forms (M1-C) |
| `waiting_room.max_tracked_sources` | integer 1–1000000 | `10000` | `RUSSH_BBS_WAITING_ROOM_MAX_TRACKED_SOURCES` | none | Rate-limiter memory bound (M1-C) |

The waiting-room banner and rules text are stored settings (D37), not config keys.

## Other flags and variables

| Name | Meaning |
|---|---|
| `--config <path>` | The config file (see above) |
| `RUSSH_BBS_CONFIG` | Same as `--config`; the flag wins |
| `RUST_LOG` | Log filter (default `info`); not a `RUSSH_BBS_*` setting |

## Implemented / Planned / Verified

- **Implemented and tested (M1-H):** all of the above; unit tests cover each layer, precedence, unknown keys, bad values with their source, the IAM refusal, redaction in `Debug`, `Display` and captured logs, default file search, the example file equalling the defaults, and this document naming every key, variable and flag.
- **Implemented and tested (M1-G):** the `postgres` datasource (D43): config wiring and the `iam_auth` refusal in the config tests, the store itself in the shared suite on Postgres 16 and on a TLS-required Postgres.
- **Planned:** IAM token authentication, a blob store reading `[blobs]`, telnet (M3), SSH password login.
- **Verified end to end:** see the M1-H pull request for the `ssh` session against a file and against environment overrides.
