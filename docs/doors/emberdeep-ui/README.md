# Emberdeep UI screenshots (M4-E3)

All made from the code in this branch.

**`real-ssh/`**: the real thing. An OpenSSH client in an 80x24 terminal (tmux, `TERM=xterm-direct` so the board composites in true colour, D109) against a running `russh-bbs-server`, signed in as the sysop and walked through one whole day: make a hero, each place in town, down the stair, a fight, an event, the map, back up. The capture is the terminal screen with its colours (`tmux capture-pane -e`) drawn to a PNG with a monospace font; a real terminal's font and exact colours differ.

**`real-ssh-16/`**: the same walk from a client that only says `TERM=screen`, so the board composites at **16 colours**, the plainest the screens will look.

**`sizes/`**: the same screens at the three terminal sizes (80x24 is `real-ssh/`; 100x30 and 132x43 here), in true colour and in the ASCII icon tier, and at the full window width where the board does not cap the box (130x39, the wide layouts). They come from `EMBERDEEP_SHOTS=dir cargo test -p russh-bbs-door-emberdeep --test screens dump_screens -- --ignored` and the same PNG drawing.

## Real ssh, 80x24, true colour

| Screen | Picture |
|---|---|
| Making a hero: a card for the calling under the cursor | ![01-create-calling](real-ssh/01-create-calling.png) |
| Making a hero: two traits from 24 | ![02-create-traits](real-ssh/02-create-traits.png) |
| Making a hero: four attribute points | ![03-create-points](real-ssh/03-create-points.png) |
| Cindermere: skyline, tavern board, the places, the way down | ![04-town](real-ssh/04-town.png) |
| Market: stalls, a 14-day shelf-price sparkline, a cost preview | ![05-market](real-ssh/05-market.png) |
| Gear: worn and spare pieces with wear | ![06-gear](real-ssh/06-gear.png) |
| Forge: repairs, recipes, a cost preview with what is left | ![07-forge](real-ssh/07-forge.png) |
| Temple | ![08-temple](real-ssh/08-temple.png) |
| Guild hall | ![09-guild](real-ssh/09-guild.png) |
| Tavern: the board, drinks, dice, the debt | ![10-tavern](real-ssh/10-tavern.png) |
| Library | ![11-library](real-ssh/11-library.png) |
| Town works: projects and their progress | ![12-works](real-ssh/12-works.png) |
| Skill tree with a detail pane | ![13-skills](real-ssh/13-skills.png) |
| Hero sheet: numbers | ![14-sheet](real-ssh/14-sheet.png) |
| Hero sheet: marks | ![15-sheet-marks](real-ssh/15-sheet-marks.png) |
| The hall: standings | ![16-hall](real-ssh/16-hall.png) |
| The hall: the dead | ![17-hall-dead](real-ssh/17-hall-dead.png) |
| The hall: what heroes changed | ![18-hall-world](real-ssh/18-hall-world.png) |
| Level 1: the lit map and the room | ![20-delve-arrival](real-ssh/20-delve-arrival.png) |
| An event scene: choices, and the ones this build cannot take with the reason | ![21-event-scene](real-ssh/21-event-scene.png) |
| Combat panel | ![22-fight](real-ssh/22-fight.png) |
| After the fight | ![23-after-fight](real-ssh/23-after-fight.png) |
| The whole map | ![24-map](real-ssh/24-map.png) |
| The sheet, opened below | ![25-sheet-delve](real-ssh/25-sheet-delve.png) |
| Deeper in the level | ![26-delve-late](real-ssh/26-delve-late.png) |
| Back in town, torches spent | ![27-back-in-town](real-ssh/27-back-in-town.png) |

## Real ssh, 80x24, 16 colours

| Screen | Picture |
|---|---|
| Cindermere: skyline, tavern board, the places, the way down | ![04-town](real-ssh-16/04-town.png) |
| Market: stalls, a 14-day shelf-price sparkline, a cost preview | ![05-market](real-ssh-16/05-market.png) |
| Forge: repairs, recipes, a cost preview with what is left | ![07-forge](real-ssh-16/07-forge.png) |
| Hero sheet: numbers | ![14-sheet](real-ssh-16/14-sheet.png) |
| The hall: standings | ![16-hall](real-ssh-16/16-hall.png) |
| An event scene: choices, and the ones this build cannot take with the reason | ![21-event-scene](real-ssh-16/21-event-scene.png) |
| Deeper in the level | ![26-delve-late](real-ssh-16/26-delve-late.png) |
| Back in town, torches spent | ![27-back-in-town](real-ssh-16/27-back-in-town.png) |

## Other sizes, tiers and depths

| File | Picture |
|---|---|
| `create-calling-80x24-ascii` | ![create-calling-80x24-ascii](sizes/create-calling-80x24-ascii.png) |
| `delve-forge-wide-130x39` | ![delve-forge-wide-130x39](sizes/delve-forge-wide-130x39.png) |
| `delve-lair-80x24-ascii` | ![delve-lair-80x24-ascii](sizes/delve-lair-80x24-ascii.png) |
| `delve-lift-runesmith-100x30-truecolor` | ![delve-lift-runesmith-100x30-truecolor](sizes/delve-lift-runesmith-100x30-truecolor.png) |
| `delve-lift-runesmith-132x43-truecolor` | ![delve-lift-runesmith-132x43-truecolor](sizes/delve-lift-runesmith-132x43-truecolor.png) |
| `delve-lift-warden-100x30-truecolor` | ![delve-lift-warden-100x30-truecolor](sizes/delve-lift-warden-100x30-truecolor.png) |
| `delve-lift-warden-132x43-truecolor` | ![delve-lift-warden-132x43-truecolor](sizes/delve-lift-warden-132x43-truecolor.png) |
| `fight-four-100x30-truecolor` | ![fight-four-100x30-truecolor](sizes/fight-four-100x30-truecolor.png) |
| `fight-four-132x43-truecolor` | ![fight-four-132x43-truecolor](sizes/fight-four-132x43-truecolor.png) |
| `fight-four-80x24-ascii` | ![fight-four-80x24-ascii](sizes/fight-four-80x24-ascii.png) |
| `fight-four-wide-130x39` | ![fight-four-wide-130x39](sizes/fight-four-wide-130x39.png) |
| `fight-guardian-100x30-truecolor` | ![fight-guardian-100x30-truecolor](sizes/fight-guardian-100x30-truecolor.png) |
| `fight-guardian-132x43-truecolor` | ![fight-guardian-132x43-truecolor](sizes/fight-guardian-132x43-truecolor.png) |
| `forge-100x30-truecolor` | ![forge-100x30-truecolor](sizes/forge-100x30-truecolor.png) |
| `forge-132x43-truecolor` | ![forge-132x43-truecolor](sizes/forge-132x43-truecolor.png) |
| `forge-80x24-ascii` | ![forge-80x24-ascii](sizes/forge-80x24-ascii.png) |
| `hall-dead-80x24-ascii` | ![hall-dead-80x24-ascii](sizes/hall-dead-80x24-ascii.png) |
| `hall-standings-100x30-truecolor` | ![hall-standings-100x30-truecolor](sizes/hall-standings-100x30-truecolor.png) |
| `hall-standings-132x43-truecolor` | ![hall-standings-132x43-truecolor](sizes/hall-standings-132x43-truecolor.png) |
| `map-100x30-truecolor` | ![map-100x30-truecolor](sizes/map-100x30-truecolor.png) |
| `map-132x43-truecolor` | ![map-132x43-truecolor](sizes/map-132x43-truecolor.png) |
| `map-80x24-ascii` | ![map-80x24-ascii](sizes/map-80x24-ascii.png) |
| `map-wide-130x39` | ![map-wide-130x39](sizes/map-wide-130x39.png) |
| `market-100x30-truecolor` | ![market-100x30-truecolor](sizes/market-100x30-truecolor.png) |
| `market-132x43-truecolor` | ![market-132x43-truecolor](sizes/market-132x43-truecolor.png) |
| `market-80x24-ascii` | ![market-80x24-ascii](sizes/market-80x24-ascii.png) |
| `sheet-numbers-100x30-truecolor` | ![sheet-numbers-100x30-truecolor](sizes/sheet-numbers-100x30-truecolor.png) |
| `sheet-numbers-132x43-truecolor` | ![sheet-numbers-132x43-truecolor](sizes/sheet-numbers-132x43-truecolor.png) |
| `skills-100x30-truecolor` | ![skills-100x30-truecolor](sizes/skills-100x30-truecolor.png) |
| `skills-132x43-truecolor` | ![skills-132x43-truecolor](sizes/skills-132x43-truecolor.png) |
| `tavern-100x30-truecolor` | ![tavern-100x30-truecolor](sizes/tavern-100x30-truecolor.png) |
| `tavern-132x43-truecolor` | ![tavern-132x43-truecolor](sizes/tavern-132x43-truecolor.png) |
| `temple-100x30-truecolor` | ![temple-100x30-truecolor](sizes/temple-100x30-truecolor.png) |
| `temple-132x43-truecolor` | ![temple-132x43-truecolor](sizes/temple-132x43-truecolor.png) |
| `town-100x30-truecolor` | ![town-100x30-truecolor](sizes/town-100x30-truecolor.png) |
| `town-132x43-truecolor` | ![town-132x43-truecolor](sizes/town-132x43-truecolor.png) |
| `town-80x24-ascii` | ![town-80x24-ascii](sizes/town-80x24-ascii.png) |
| `town-wide-130x39` | ![town-wide-130x39](sizes/town-wide-130x39.png) |
| `works-100x30-truecolor` | ![works-100x30-truecolor](sizes/works-100x30-truecolor.png) |
| `works-132x43-truecolor` | ![works-132x43-truecolor](sizes/works-132x43-truecolor.png) |
