# russh-bbs guides

Two guides for people who run or use a board. They describe what the software does **today**; anything not built yet is marked **Not built** and, where there is one, links to the milestone that plans it ([features.md](../features.md) is the full register).

| Guide | For | Start here |
|---|---|---|
| **Sysop guide** | The person who installs, configures and looks after a board | [sysop/01-install.md](sysop/01-install.md) |
| **Member guide** | Someone with (or about to get) an account | [member/01-connect.md](member/01-connect.md) |
| **Privacy: who can read what** | Everyone. Said once here, not on every page | [privacy.md](privacy.md) |

## The sysop guide

1. [Install](sysop/01-install.md): the binary, the container image, SQLite or Postgres, and the TLS-required Postgres setup
2. [First start and claiming the board](sysop/02-first-start.md)
3. [Configuration, a guided tour](sysop/03-config.md)
4. [People: registration modes, roles and ranks, moderation, telnet's limits](sysop/04-people.md)
5. [Look and feel: styles, layouts, board identity, art and theme packs, `menus.toml` and pages, widgets and locks](sysop/05-look.md)
6. [Doors and the blob store](sysop/06-doors-and-storage.md)
7. [Running it: backups, upgrades, troubleshooting](sysop/07-operate.md)

## The member guide

1. [Connecting and signing in](member/01-connect.md): ssh, ssh without a key, telnet; password first, keys as the extra; device links and recovery codes
2. [Your terminal](member/02-terminal.md): styles, layouts, Blocks borders, the Terminal.app tip, mouse, motion and accessibility
3. [Using the board](member/03-using-the-board.md): forums, mail, chat (and split chat with `Ctrl-T`), community, widgets, the `Ctrl-K` palette
4. [Doors](member/04-doors.md)

## How this guide was checked

Every command and screen in these pages was run on **2026-11-06** from a clean data directory: the release binary built from this branch's base (`5b7a85c`), a real OpenSSH 9.6 client (`ssh`) and a real `telnet` client (inetutils), driven in `tmux`. Screenshots in [img/](img/) were taken from those sessions with [`tools/screenshots/`](../../tools/screenshots/README.md).

What was **not** verified, so the pages that touch it say so:

- **The container image.** There is no Docker daemon in the environment the guide was written in, so `docker run`, `compose.yaml` and the volume steps are taken from [deploy.md](../deploy.md), not walked. The image itself is not published yet ([deploy.md](../deploy.md#owner-action-release)).
- **S3 blobs.** No S3 or MinIO server was available. The `[blobs]` keys are quoted from [config.md](../config.md); the local backend starts and creates its directory, and nothing uses the blob store yet (file areas are M4-F2).
- **Terminals other than tmux/xterm-class.** macOS Terminal.app, iTerm2, Windows Terminal and screen readers were not tried.
- **Upgrades across versions.** There is only one version of the binary, so "run the new binary on the old database" was not exercised; migrations running on a fresh database were (SQLite and Postgres 16).

Behaviour found while writing that disagrees with another document is listed as FIX rows in [coordination.md](../coordination.md), not fixed here.
