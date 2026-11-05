# FIX-M35-2 and FIX-M35-4 walk

Real OpenSSH 9.6 against the debug build of `russh-bbs-server`, a scratch board (telnet on), founder claimed, icon style set to ASCII with colour off, driven in tmux panes through `docs/qa/m35-screens/tools/` (`claim.py`, `drive.py`, `lib.py`) and read with `pyte`. Screenshots by `tools/screenshots/shot.py`.

| Window | Menu cells | Scrollbar | Image |
|---|---|---|---|
| 40x12 | all ASCII | `#` thumb, `|` track | [menu-ascii-40x12.png](menu-ascii-40x12.png) |
| 80x14 | all ASCII | `#` thumb, `|` track | [menu-ascii-80x14.png](menu-ascii-80x14.png) |
| 80x24 | all ASCII (the menu fits, no scrollbar) | none | [menu-ascii-80x24.png](menu-ascii-80x24.png) |

**Not verified:** the wire. The harness's raw capture includes the sign-in screens before the tier is set, which carry bytes >= 0x80, so I checked cells, not wire bytes. Also not walked over ssh: the door selection bar (L1) and the locked-layout notice (L3); both are covered by unit tests only.
