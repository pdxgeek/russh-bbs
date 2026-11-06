# README screenshots

The images in this folder are used by the front-page [README](../../README.md). They total well under a megabyte.

How they were made (DOC-2):

- The real release build of `russh-bbs-server` ran against a scratch data directory, on a loopback port, with `RUSSH_BBS_BOARD_NAME=Chatsubo`, `RUSSH_BBS_BOARD_STYLE` (`phosphor`, `neon` or `paper`) and `RUSSH_BBS_BOARD_LAYOUT=dashboard`.
- The board was claimed as the founder with `ssh claim@...`, then driven by OpenSSH 9.6 (`ssh -tt`, `TERM=xterm-256color`, `COLORTERM=truecolor`) through a pty. The sample content (four forum threads, a few replies, two one-liners, a few chat lines) was typed in over the same ssh session. All of it is written for this purpose; the board has one member, the founder.
- Each screen was read through the `pyte` terminal emulator and drawn to PNG by the shared renderer, [`tools/screenshots`](../../tools/screenshots/README.md) (`shot.render_pyte`), at 132x43 (forum screens at 100x30).
- Doors are shown at their first screens (new hero, new clan, enrolment, the bridge after signing the loan, and the Nullroute home). Nothing is mocked or edited.

The key script is not part of the board. To remake a shot, start a scratch board as above, drive it with a pty walk like [`docs/qa/m4-s3-screens/walk.py`](../qa/m4-s3-screens/walk.py), and call `shot.render_pyte(term.screen, "out.png")`.
