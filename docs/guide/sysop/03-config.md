# 3. Configuration, a guided tour

[config.md](../../config.md) is the reference: every key, its type, default, environment variable and flag. This page is the tour: where the settings are, what to set first, and what the server does when you get one wrong. [`config.example.toml`](../../../config.example.toml) lists every key at its default (a test keeps it equal to the real defaults), so copy it and delete what you do not change.

## How settings combine

Lowest to highest: **built-in defaults, the config file, `RUSSH_BBS_*` environment variables, command-line flags.** A key set higher replaces the same key lower down.

The file is `--config <path>` (or `RUSSH_BBS_CONFIG`; a file you *name* must exist). With neither, the first of these that exists is read: `$XDG_CONFIG_HOME/russh-bbs/config.toml` (or `~/.config/russh-bbs/config.toml`), then `/data/config.toml` (the container's volume). No file at all is fine: the board runs on defaults.

> Put `config.toml` where you like, but `menus.toml`, `pages/`, `themes/`, `art/` and `packs/` are looked for **beside the host key**, in the data directory ([05-look.md](05-look.md)). In a normal install that is the same place as everything else: `~/.local/share/russh-bbs/`.

## Unknown or wrong settings stop the boot

This is deliberate: a typo must not silently do nothing. Each of these was run:

| You did | The server says (and exits 1) |
|---|---|
| Typed a key wrong in the file | `unknown key `ssh.listen_typo` in config file /…/bad.toml` |
| Set an environment variable no setting reads | `unknown environment variable RUSSH_BBS_NOPE: no setting reads it` |
| Named a config file that is not there | `config file /nope.toml does not exist (it was named explicitly)` |
| Set `telnet.allow_login` without `telnet.enabled` | `invalid value for `telnet.allow_login` from environment variable RUSSH_BBS_TELNET_ALLOW_LOGIN: needs telnet.enabled = true; login over telnet is a second opt-in (D27)` |
| Started a second copy on the same port | `cannot listen on 127.0.0.1:22230: Address already in use` |
| Left the host key readable by others | `host key file …/host_ed25519_key has mode 644; it must not be accessible to group or others (chmod 600)` |

(Kubernetes service-link variables such as `RUSSH_BBS_PORT` are the one exception: ignored with a warning, or set `enableServiceLinks: false`. [config.md](../../config.md#kubernetes-service-links).) Settings that parse but lower the board's security log a warning at start, so read the log after any change.

## The tour, in the order a sysop needs it

**Listening.** `ssh.listen` (default `0.0.0.0:2222`; flag `--listen`). Members must reach this port. Do not run it as root on port 22; forward the port instead.

**Where data lives.** `datasource.*` ([install](01-install.md#sqlite-or-postgres)), `ssh.host_key`, `mail.key_file`, `blobs.*`.

**Who can join.** `registration_mode` is `invite` (the default), `apply` or `open` ([04-people.md](04-people.md#registration-modes)). It is stored in the database: if the file, the environment or a flag sets it, that value is **written to the database at every start and wins**; if none sets it, the stored value stands. So a board you set to `open` once with an environment variable stays `open` until a start with another value. There is no screen for it yet, so decide it in config. The `open` daily cap (`open_daily_cap`, default 25) and the invite-request cap (`queue_max_pending`, default 200) are **database settings with no config key and no screen**; [config.md](../../config.md#waiting_room-d37) gives the SQL.

**Telnet.** Off by default and binds no port. `telnet.enabled`, then `telnet.allow_login` for password login and the waiting room; `telnet.listen` defaults to `0.0.0.0:2323`. Turning it on puts the board in **low-security mode** ([04-people.md](04-people.md#telnet-and-its-limits)).

**Who can overload it.** `ssh.max_connections` (64), `ssh.render_cols`/`render_rows` (the memory ceiling is connections × render area; it is logged at start as `render_memory_ceiling_mib`: 585 MiB at the defaults, so lower the render cap on a small host), `waiting_room.*` (per-address connection and per-key attempt rates), and the `ssh.requests_per_second`, `input_bytes_per_second` and `flood_strikes` bounds. Defaults are sensible; change them only with a reason.

**Password login over ssh.** `ssh.allow_password_login` is **on by default**: `ssh handle@host` signs in with a password unless a key bound to that handle signs in first. Off, a member who has only a password can sign in over ssh only by key. Wrong passwords back off per handle and per network; a key is never held by that ([member guide](../member/01-connect.md#the-sign-in-model)).

**The look.** `[board]` and `[widgets]` ([05-look.md](05-look.md)).

**Feeds (RSS in and out).** `feeds.enabled` is off: the server makes **no outbound request** until you turn it on. It never uses a proxy. RSS out is a separate plain-HTTP listener (`feeds.rss_out_enabled`, with a base URL). See [config.md](../../config.md#feeds-d89); not walked in this guide.

## A small config that was run

This file was used for the walks in these guides (telnet on, a named board, a dashboard layout, a message of the day):

```toml
registration_mode = "invite"

[telnet]
enabled = true
allow_login = true
listen = "127.0.0.1:23232"

[board]
name = "Night Owl BBS"
tagline = "Late hours, good company."
location = "Portland, OR"
since = "2026"
layout = "dashboard"
style = "phosphor"
quotes = "Keep it civil.|See you next time."
ticker = "Welcome to Night Owl|Be kind to each other"

[widgets]
motd = "Sysop is away until Monday."
```

At start the log said `telnet.enabled is on: the board is in LOW-SECURITY MODE` and the staff screens gained the low-security marker (drawn as `⟦⊖⟧ telnet is on` in the Unicode icon tier; the docs call it `LOW-SECURITY`). Every environment variable is the key in capitals with `_` for `.`: `RUSSH_BBS_BOARD_NAME`, `RUSSH_BBS_TELNET_ENABLED`; the older names `RUSSH_BBS_LISTEN`, `RUSSH_BBS_HOST_KEY` and `RUSSH_BBS_DB` are kept.

## Secrets

`datasource.url` carries a password. It has no command-line flag, is masked in logs and errors, and the server warns if a config file that sets it is readable by group or others (`chmod 600`). S3 credentials are never read from the config file: they come from `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY` (or the platform's own role). Nothing in the repository holds a credential.

Next: [people](04-people.md).
