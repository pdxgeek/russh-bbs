# M4-S3 walk: the Saltmarch screens over real ssh

Date: 2026-11-04. Branch `m4/saltmarch-ui`. **What was run:** the real server binary (`target/debug/russh-bbs-server`, SQLite, a fresh board each time) claimed with the OpenSSH 9.6 client (`ssh claim@127.0.0.1` with a throwaway ed25519 key, the token printed on the console), then signed in as the founder with `ssh -tt` in a pty of 80x24 and of 132x43. After the icon check (`y` for the Nerd Font tier with colour, or `n` twice for plain ASCII with no colour) the walk opened `d` Doors, Saltmarch, founded a clan and pressed the keys that reach each screen. What a terminal emulator (`pyte`) showed after each step is in the `.txt` files; the same screen with the colours the server sent is drawn to a PNG by `render.py` from the emulator's cells. The script is `docs/qa/m4-s3-screens/walk.py` (it was written for this walk and is kept so it can be repeated: `python3 walk.py claim|play|report ...`).

The morning report needs the day to change, so the board was stopped after the walk and started again on the same database under `faketime -f "+2d"`, and the founder opened the door again.

## What the walk found out about the board

- **A door in an 80x24 window gets 74 columns by 20 rows** (the box and the two heading rows take the rest; measured from the capture). In a 132x43 window the width is the same 74 (the classic layout's box is 76 wide) and the height is 37. So today the real door area is what the screens call *Tiny* (under 78 columns or 22 rows) or *Compact* at best; *Roomy* and *Grand* are drawn only by the tests until another layout gives a door more room. The tests cover 74x18, 76x19 and 60x16 as well as the three sizes the owner named.
- **The board composites doors at 16 colours whatever the terminal** (`server::doors::draw::depth`, D104). The ten distinct colours in the 80x24 and 132x43 captures are all among the sixteen names (black, dark grey, grey, red, green, yellow, bright yellow, blue, bright blue, cyan), and no `truecolor` sequence was sent. That is why the accents were chosen so that the nearest of the sixteen is the one intended (D-TBD (3)): the first walk showed every muted accent turning into dark grey. The pictures in `design-132x43/` are the same screens at truecolor, which is how they will look when the member setting has a depth.
- **ASCII tier, colour off:** the capture is ASCII only and the raw stream held **no colour sequence at all** (0 `38;5` and 0 `38;2`); the tide is `-` and `~` on the map and `^^ FLOOD ^^` on the strip.
- **No bold-plus-reverse (the reserved marker style) reached the screen** from the door in any capture (0 cells in the colour captures).

## Captures

Each folder has `NAME.png` and `NAME.txt` for these stops (the numbers are the order they were reached in):

| Stop | 80x24, 16 colours | 80x24, ASCII, no colour | 132x43, 16 colours |
|---|---|---|---|
| 01 title and naming | [png](m4-s3-screens/ssh-80x24/01-naming.png) | [png](m4-s3-screens/ssh-ascii-80x24/01-naming.png) | [png](m4-s3-screens/ssh-132x43/01-naming.png) |
| 02 home | [png](m4-s3-screens/ssh-80x24/02-home.png) | [png](m4-s3-screens/ssh-ascii-80x24/02-home.png) | [png](m4-s3-screens/ssh-132x43/02-home.png) |
| 03 hold | [png](m4-s3-screens/ssh-80x24/03-hold.png) | [png](m4-s3-screens/ssh-ascii-80x24/03-hold.png) | [png](m4-s3-screens/ssh-132x43/03-hold.png) |
| 04, 05 crew and a crew member (Down) | [png](m4-s3-screens/ssh-80x24/05-crew-detail.png) | [png](m4-s3-screens/ssh-ascii-80x24/05-crew-detail.png) | [png](m4-s3-screens/ssh-132x43/05-crew-detail.png) |
| 06 market | [png](m4-s3-screens/ssh-80x24/06-market.png) | [png](m4-s3-screens/ssh-ascii-80x24/06-market.png) | [png](m4-s3-screens/ssh-132x43/06-market.png) |
| 07 quay (the tide table) | [png](m4-s3-screens/ssh-80x24/07-quay.png) | [png](m4-s3-screens/ssh-ascii-80x24/07-quay.png) | [png](m4-s3-screens/ssh-132x43/07-quay.png) |
| 08 marsh chart (arrows pan) | [png](m4-s3-screens/ssh-80x24/08-chart.png) | [png](m4-s3-screens/ssh-ascii-80x24/08-chart.png) | [png](m4-s3-screens/ssh-132x43/08-chart.png) |
| 09 Tidecourt | [png](m4-s3-screens/ssh-80x24/09-tidecourt.png) | [png](m4-s3-screens/ssh-ascii-80x24/09-tidecourt.png) | [png](m4-s3-screens/ssh-132x43/09-tidecourt.png) |
| 10 league, 11 hall, 12 post | [10](m4-s3-screens/ssh-80x24/10-league.png) [11](m4-s3-screens/ssh-80x24/11-hall.png) [12](m4-s3-screens/ssh-80x24/12-post.png) | [10](m4-s3-screens/ssh-ascii-80x24/10-league.png) [11](m4-s3-screens/ssh-ascii-80x24/11-hall.png) [12](m4-s3-screens/ssh-ascii-80x24/12-post.png) | [10](m4-s3-screens/ssh-132x43/10-league.png) [11](m4-s3-screens/ssh-132x43/11-hall.png) [12](m4-s3-screens/ssh-132x43/12-post.png) |
| 13 expedition planner (party, kit, target, cost preview) | [png](m4-s3-screens/ssh-80x24/13-planner.png) | [png](m4-s3-screens/ssh-ascii-80x24/13-planner.png) | [png](m4-s3-screens/ssh-132x43/13-planner.png) |
| 14 marsh map on a trip, 14b after arrow steps | [png](m4-s3-screens/ssh-80x24/14-marsh.png) | [png](m4-s3-screens/ssh-ascii-80x24/14-marsh.png) | [png](m4-s3-screens/ssh-132x43/14-marsh.png) |
| 15 event scene, 15b after the choice | [png](m4-s3-screens/ssh-80x24/15-scene.png) | [png](m4-s3-screens/ssh-ascii-80x24/15-scene.png) | [png](m4-s3-screens/ssh-132x43/15-scene.png) |
| 16 skirmish, 16b after a quick resolve | [png](m4-s3-screens/ssh-80x24/16-skirmish.png) | [png](m4-s3-screens/ssh-ascii-80x24/16-skirmish.png) | [png](m4-s3-screens/ssh-132x43/16-skirmish.png) |
| 17, 18 the marsh later, home again | [17](m4-s3-screens/ssh-80x24/17-marsh-later.png) [18](m4-s3-screens/ssh-80x24/18-home-after.png) | [17](m4-s3-screens/ssh-ascii-80x24/17-marsh-later.png) [18](m4-s3-screens/ssh-ascii-80x24/18-home-after.png) | [17](m4-s3-screens/ssh-132x43/17-marsh-later.png) [18](m4-s3-screens/ssh-132x43/18-home-after.png) |

The morning report, the morning after (`faketime +2d`): [19](m4-s3-screens/ssh-next-morning/19-morning-report.png), and the home screen after it: [20](m4-s3-screens/ssh-next-morning/20-home-next-morning.png).

Truecolor designs from the test harness (the same screens at the door sizes the owner named, drawn as the screens will look with a colour depth; not captured over ssh): [design-132x43/](m4-s3-screens/design-132x43/) has all sixteen, for example [the marsh](m4-s3-screens/design-132x43/marsh.png), [the report](m4-s3-screens/design-132x43/report.png), [the market](m4-s3-screens/design-132x43/market.png), [the skirmish](m4-s3-screens/design-132x43/fight.png) and [a scene](m4-s3-screens/design-132x43/scene.png).

## Things the walk checked

- The same keys work as in the debug screens: letters, and now Up, Down and Enter. On the marsh the arrow keys took a step (the party's tile changed in the next capture), on the chart they panned it, on the crew screen Down chose the next crew member.
- Scenes show each option's trade, its chance to go badly as a number and a warning on an option that can start a fight; the walk took the way that starts a fight when the scene offered one, and a skirmish came up in the 80x24, ASCII and 132x43 walks (the screens are in the table above).
- The morning report after two nights: what was made, what healed, raids and letters (none), the day's tide chart with the flood line, the flood windows and the standings.
- A clan with a spring-tide flood warning showed the `FLOOD` band in words and colour on every screen.

## Not walked, and what was not exercised

- **Telnet.** The screens draw through the same `Driver` as ssh and the repo's telnet client was not used.
- **A truecolor terminal.** The board sends 16 colours; the truecolor pictures are from the test harness.
- **A terminal that is not xterm-like**, a real Nerd Font installed (the Nerd Font tier draws the Unicode glyphs, see D-TBD (2)) and a screen reader driving the look-around (no key is bound to it yet; the text is tested, not heard).
- **A door area larger than 74x37.** The Roomy and Grand layouts (100x30, 132x43 door areas) are covered by the tests and the truecolor pictures, not by a real window, because the classic layout's box limits the door.
- **The Postgres suites** (this slice touches no store code).
- The skirmish and scene came up by chance on the marsh (about one scene a walk and a skirmish in a minority of walks); several walks had none, so those were repeated until the screens were captured. The numbers of walks that had them are not a measure of how often they occur.
