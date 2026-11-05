# UI-6 walk: sysop menus and pages

Date: 2026-11-05. Branch `m35/sysop-menus`. **What was run:** the real server binary (`target/debug/russh-bbs-server`, SQLite, a scratch directory, style `neon`, layout `dashboard`) with a `menus.toml` and two pages in the data directory, driven by OpenSSH 9.6 (`ssh -tt`, `TERM=xterm-256color`) inside `tmux`, screens rendered by `tools/screenshots/shot.py`. The founder claimed the board, made an invite, and a second key redeemed it as the member `marlowe`. Scripts: [walk.py](ui-6-screens/walk.py) (sysop, pages, invite) and [walk_part2.py](ui-6-screens/walk_part2.py) (member, Menus and pages, hostile file, reload). The member sign-up was typed by hand between the two scripts (`ssh new@host`, Redeem, handle, password twice).

## What the walk showed

- **Reordered, renamed, hidden, nested (10, 11, 20).** The menu is "Night Owl Hall": House rules (a page) first, `[x] Messages` (Mail renamed and re-keyed), `[t] Talk and play` (a group holding Chat, Doors and a page), Files hidden. Sysop sees the Sysop item as before; the member (20) does not.
- **Hidden and foreign keys do nothing (21).** As the member, `a` (Files, hidden) and `o` (Sysop tools, not theirs) changed nothing.
- **Pages (12, 13, 23).** The history page has art (an owl) above a bordered text panel with a heading rule, bullets and the arrows hint; the rules page has a quotation and a rule. `b` returns to the same menu. The marker row stays the frame's.
- **Menus and pages screen (30 to 35).** Sysop tools, `m`. 31: the effective menu for the member role (`r` changes role), with `>` for submenus and `*` for pages. 32/33: `t` to the pages, Enter shows one as a member sees it. 34: a `menus.toml` with an escape sequence in a label was refused on `l` ("menus.toml refused, built-in menu in use") and the built-in menu came back. 35: the fixed file reloaded ("New sessions get it; people signed in keep their current menu").

## Not walked

- The audit rows (asserted by the in-process ssh test in `access_paths.rs`), telnet, the ASCII tier over ssh, other terminals, Postgres (the store was not touched).
- A second member signing in after the reload (also asserted by the in-process test).
- The refusal line is clipped at the screen width; the full reason is in the server log.
