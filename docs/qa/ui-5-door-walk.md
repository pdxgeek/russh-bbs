# UI-5 walk: the example door over real ssh and telnet

Date: 2026-11-04. Branch `m35/door-ratatui`. **What was run:** the real server binary (`target/debug/russh-bbs-server`, SQLite, telnet on with password login) on an empty board, claimed with the OpenSSH client (`ssh claim@127.0.0.1` with a throwaway ed25519 key), then signed in as the founder with the OpenSSH client (`ssh -tt`, a pty of 80x24 and 132x43) and with the stock `telnet` client (80x24). The screens below are what a terminal emulator (`pyte`) showed after the keys were sent from a pty; the key script and the emulator were not part of the repo. Menu path: `Main menu`, `d` (Doors), `1` (Coin Toss), then `h` and `t` calls, `q` to leave. The board was rebuilt for each ssh capture because the door allows five tosses a day and keeps the save.

**What this shows and what it does not.** It shows the example door's table, gauge, sparkline and braille chart reaching a real terminal over both transports, in the Nerd Font, Unicode and ASCII tiers, with the ASCII tier drawing only ASCII. It does not show colour: `pyte` output above is text only. The colour check is on the raw bytes the server sent (SGR sequences), tallied below. The 132x43 window still shows the classic layout's 80-column box, centered, so the door's area there is the same width as at 80x24 (and taller); the 132x43 *door area* is covered by `crates/doors/example/tests/screens.rs`, which composites the door at those sizes directly. A door inside a window of a different size, a terminal that is not xterm-like, and a Nerd Font actually installed were not tried.

## Raw bytes on the wire (SGR sequences counted from each session)

| Capture | truecolor (`38;2` / `48;2`) | 256-colour (`38;5` / `48;5`) | bold and reverse in one sequence |
|---|---|---|---|
| ssh 80x24, Unicode, colour on | 0 | 47 | 0 |
| ssh 132x43, Unicode, colour on | 0 | 55 | 0 |
| ssh 80x24, Nerd Font, colour on | 0 | 47 | 0 |
| ssh 80x24, ASCII, colour off | 0 | 0 | 0 |
| ssh 132x43, ASCII, colour off | 0 | 0 | 0 |
| telnet 80x24, Nerd Font, colour on | 0 | 43 | 8 |

No truecolor is sent. The door's own amber and teal are `Rgb` colours; after compositing at 16 colours they reached the terminal as `38;5;N` sequences whose indexes were only 2, 3 and 6 (green, yellow, cyan, all among the sixteen names), checked on one extra 80x24 ssh session. Nothing is coloured when the member has colour off. The eight bold-plus-reverse sequences on telnet are the board's own `UNENCRYPTED` footer row, the reserved chrome style (D39), which is drawn outside the door's area; the ssh sessions have no footer and no such sequence.

### ssh 80x24, Unicode tier

```
--- door just opened
  ┌─────────────────────────────── russh-bbs ────────────────────────────────┐
  │Coin Toss                                                                 │
  │                                                                          │
  │┌ founder - day 20761 ───────────────────────────────────────────────────┐│
  ││Coins: 5   Tosses left today: 5   Streak: 0 (best 0)                    ││
  ││                         0/5 tosses used today                          ││
  ││┌ This visit ────────────────────────────┐┌ Streak ────────────────────┐││
  │││Call heads or tails to start.           ││                            │││
  │││                                        │└────────────────────────────┘││
  │││                                        │┌ Purse ─────────────────────┐││
  │││                                        ││                            │││
  │││                                        ││                            │││
  │││                                        ││                            │││
  │││                                        ││                            │││
  │││                                        ││                            │││
  │││                                        ││                            │││
  │││                                        ││                            │││
  │││                                        ││                            │││
  ││└────────────────────────────────────────┘└────────────────────────────┘││
  ││[h] call heads   [t] call tails   [n] change name   [q] leave           ││
  ││The board has tossed 0 coins today and 0 yesterday.                     ││
  ││Days the board has been keeping count: 1.                               ││
  │└────────────────────────────────────────────────────────────────────────┘│
  └──────────────────────────────────────────────────────────────────────────┘

--- after 2 toss(es)
  ┌─────────────────────────────── russh-bbs ────────────────────────────────┐
  │Coin Toss                                                                 │
  │                                                                          │
  │┌ founder - day 20761 ───────────────────────────────────────────────────┐│
  ││Coins: 3   Tosses left today: 3   Streak: 0 (best 0)                    ││
  ││█████████████████████████2/5 tosses used today                          ││
  ││┌ This visit ────────────────────────────┐┌ Streak ────────────────────┐││
  │││#   Call   Result Coins                 ││                            │││
  │││1   heads  lost   4                     │└────────────────────────────┘││
  │││2   tails  lost   3                     │┌ Purse ─────────────────────┐││
  │││                                        ││                            │││
  │││                                        ││⣀⡀                          │││
  │││                                        ││ ⠈⠉⠑⠒⠢⠤⢄⣀⡀                  │││
  │││                                        ││         ⠈⠉⠑⠒⠢⠤⢄⣀⡀          │││
  │││                                        ││                 ⠈⠉⠉⠒⠒⠤⠤⢄⣀⡀ │││
  │││                                        ││                          ⠈⠉│││
  │││                                        ││                            │││
  ││└────────────────────────────────────────┘└────────────────────────────┘││
  ││[h] call heads   [t] call tails   [n] change name   [q] leave           ││
  ││You called tails. -1 coin. A small, final clink. The house keeps it.    ││
  ││The board has tossed 2 coins today and 0 yesterday.                     ││
  ││Days the board has been keeping count: 1.                               ││
  │└────────────────────────────────────────────────────────────────────────┘│
  └──────────────────────────────────────────────────────────────────────────┘
```

### ssh 132x43, Unicode tier

```
--- door just opened

                            ┌─────────────────────────────── russh-bbs ────────────────────────────────┐
                            │Coin Toss                                                                 │
                            │                                                                          │
                            │┌ founder - day 20761 ───────────────────────────────────────────────────┐│
                            ││Coins: 5   Tosses left today: 5   Streak: 0 (best 0)                    ││
                            ││                         0/5 tosses used today                          ││
                            ││┌ This visit ────────────────────────────┐┌ Streak ────────────────────┐││
                            │││Call heads or tails to start.           ││                            │││
                            │││                                        │└────────────────────────────┘││
                            │││                                        │┌ Purse ─────────────────────┐││
                            │││                                        ││                            │││
                            │││                                        ││                            │││
                            │││                                        ││                            │││
                            │││                                        ││                            │││
                            │││                                        ││                            │││
                            │││                                        ││                            │││
                            │││                                        ││                            │││
                            │││                                        ││                            │││
                            │││                                        ││                            │││
                            │││                                        ││                            │││
                            │││                                        ││                            │││
                            │││                                        ││                            │││
                            │││                                        ││                            │││
                            │││                                        ││                            │││
                            │││                                        ││                            │││
                            │││                                        ││                            │││
                            │││                                        ││                            │││
                            │││                                        ││                            │││
                            │││                                        ││                            │││
                            │││                                        ││                            │││
                            │││                                        ││                            │││
                            │││                                        ││                            │││
                            │││                                        ││                            │││
                            │││                                        ││                            │││
                            │││                                        ││                            │││
                            ││└────────────────────────────────────────┘└────────────────────────────┘││
                            ││[h] call heads   [t] call tails   [n] change name   [q] leave           ││
                            ││The board has tossed 0 coins today and 0 yesterday.                     ││
                            ││Days the board has been keeping count: 1.                               ││
                            │└────────────────────────────────────────────────────────────────────────┘│
                            └──────────────────────────────────────────────────────────────────────────┘

--- after 2 toss(es)

                            ┌─────────────────────────────── russh-bbs ────────────────────────────────┐
                            │Coin Toss                                                                 │
                            │                                                                          │
                            │┌ founder - day 20761 ───────────────────────────────────────────────────┐│
                            ││Coins: 5   Tosses left today: 3   Streak: 1 (best 1)                    ││
                            ││█████████████████████████2/5 tosses used today                          ││
                            ││┌ This visit ────────────────────────────┐┌ Streak ────────────────────┐││
                            │││#   Call   Result Coins                 ││ █                          │││
                            │││1   tails  lost   4                     │└────────────────────────────┘││
                            │││2   heads  won    5                     │┌ Purse ─────────────────────┐││
                            │││                                        ││                            │││
                            │││                                        ││                            │││
                            │││                                        ││                            │││
                            │││                                        ││                            │││
                            │││                                        ││                            │││
                            │││                                        ││                            │││
                            │││                                        ││                            │││
                            │││                                        ││                            │││
                            │││                                        ││⠑⢄                        ⡠⠊│││
                            │││                                        ││  ⠱⡀                    ⢀⠔⠁ │││
                            │││                                        ││   ⠈⠢⡀                ⢀⠔⠁   │││
                            │││                                        ││     ⠈⠢⡀             ⡠⠊     │││
                            │││                                        ││       ⠑⢄          ⡠⠊       │││
                            │││                                        ││         ⠑⢄      ⢀⠔⠁        │││
                            │││                                        ││           ⠱⡀  ⢀⠔⠁          │││
                            │││                                        ││            ⠈⠢⡠⠊            │││
                            │││                                        ││                            │││
                            │││                                        ││                            │││
                            │││                                        ││                            │││
                            │││                                        ││                            │││
                            │││                                        ││                            │││
                            │││                                        ││                            │││
                            │││                                        ││                            │││
                            │││                                        ││                            │││
                            ││└────────────────────────────────────────┘└────────────────────────────┘││
                            ││[h] call heads   [t] call tails   [n] change name   [q] leave           ││
                            ││You called heads. +1 coin. The coin hums as it lands your way.          ││
                            ││The board has tossed 2 coins today and 0 yesterday.                     ││
                            ││Days the board has been keeping count: 1.                               ││
                            │└────────────────────────────────────────────────────────────────────────┘│
                            └──────────────────────────────────────────────────────────────────────────┘
```

### ssh 80x24, ASCII tier, colour off

```
--- door just opened
  +------------------------------- russh-bbs --------------------------------+
  |Coin Toss                                                                 |
  |                                                                          |
  |+ founder - day 20761 ---------------------------------------------------+|
  ||Coins: 5   Tosses left today: 5   Streak: 0 (best 0)                    ||
  ||                         0/5 tosses used today                          ||
  ||+ This visit ----------------------------++ Streak --------------------+||
  |||Call heads or tails to start.           ||                            |||
  |||                                        |+----------------------------+||
  |||                                        |+ Purse ---------------------+||
  |||                                        ||                            |||
  |||                                        ||                            |||
  |||                                        ||                            |||
  |||                                        ||                            |||
  |||                                        ||                            |||
  |||                                        ||                            |||
  |||                                        ||                            |||
  |||                                        ||                            |||
  ||+----------------------------------------++----------------------------+||
  ||[h] call heads   [t] call tails   [n] change name   [q] leave           ||
  ||The board has tossed 0 coins today and 0 yesterday.                     ||
  ||Days the board has been keeping count: 1.                               ||
  |+------------------------------------------------------------------------+|
  +--------------------------------------------------------------------------+

--- after 1 toss(es)
  +------------------------------- russh-bbs --------------------------------+
  |Coin Toss                                                                 |
  |                                                                          |
  |+ founder - day 20761 ---------------------------------------------------+|
  ||Coins: 6   Tosses left today: 4   Streak: 1 (best 1)                    ||
  ||##############=          1/5 tosses used today                          ||
  ||+ This visit ----------------------------++ Streak --------------------+||
  |||#   Call   Result Coins                 ||#                           |||
  |||1   heads  won    6                     |+----------------------------+||
  |||                                        |+ Purse ---------------------+||
  |||                                        ||                            |||
  |||                                        ||                            |||
  |||                                        ||                    ........|||
  |||                                        ||        ............        |||
  |||                                        ||........                    |||
  |||                                        ||                            |||
  |||                                        ||                            |||
  ||+----------------------------------------++----------------------------+||
  ||[h] call heads   [t] call tails   [n] change name   [q] leave           ||
  ||You called heads. +1 coin. It rings on the table and stays put. Yours.  ||
  ||The board has tossed 1 coins today and 0 yesterday.                     ||
  ||Days the board has been keeping count: 1.                               ||
  |+------------------------------------------------------------------------+|
  +--------------------------------------------------------------------------+
```

### ssh 132x43, ASCII tier, colour off

```
--- door just opened

                            +------------------------------- russh-bbs --------------------------------+
                            |Coin Toss                                                                 |
                            |                                                                          |
                            |+ founder - day 20761 ---------------------------------------------------+|
                            ||Coins: 5   Tosses left today: 5   Streak: 0 (best 0)                    ||
                            ||                         0/5 tosses used today                          ||
                            ||+ This visit ----------------------------++ Streak --------------------+||
                            |||Call heads or tails to start.           ||                            |||
                            |||                                        |+----------------------------+||
                            |||                                        |+ Purse ---------------------+||
                            |||                                        ||                            |||
                            |||                                        ||                            |||
                            |||                                        ||                            |||
                            |||                                        ||                            |||
                            |||                                        ||                            |||
                            |||                                        ||                            |||
                            |||                                        ||                            |||
                            |||                                        ||                            |||
                            |||                                        ||                            |||
                            |||                                        ||                            |||
                            |||                                        ||                            |||
                            |||                                        ||                            |||
                            |||                                        ||                            |||
                            |||                                        ||                            |||
                            |||                                        ||                            |||
                            |||                                        ||                            |||
                            |||                                        ||                            |||
                            |||                                        ||                            |||
                            |||                                        ||                            |||
                            |||                                        ||                            |||
                            |||                                        ||                            |||
                            |||                                        ||                            |||
                            |||                                        ||                            |||
                            |||                                        ||                            |||
                            |||                                        ||                            |||
                            ||+----------------------------------------++----------------------------+||
                            ||[h] call heads   [t] call tails   [n] change name   [q] leave           ||
                            ||The board has tossed 0 coins today and 0 yesterday.                     ||
                            ||Days the board has been keeping count: 1.                               ||
                            |+------------------------------------------------------------------------+|
                            +--------------------------------------------------------------------------+

--- after 1 toss(es)

                            +------------------------------- russh-bbs --------------------------------+
                            |Coin Toss                                                                 |
                            |                                                                          |
                            |+ founder - day 20761 ---------------------------------------------------+|
                            ||Coins: 6   Tosses left today: 4   Streak: 1 (best 1)                    ||
                            ||##############=          1/5 tosses used today                          ||
                            ||+ This visit ----------------------------++ Streak --------------------+||
                            |||#   Call   Result Coins                 ||#                           |||
                            |||1   tails  won    6                     |+----------------------------+||
                            |||                                        |+ Purse ---------------------+||
                            |||                                        ||                            |||
                            |||                                        ||                            |||
                            |||                                        ||                            |||
                            |||                                        ||                            |||
                            |||                                        ||                            |||
                            |||                                        ||                            |||
                            |||                                        ||                            |||
                            |||                                        ||                            |||
                            |||                                        ||                        ....|||
                            |||                                        ||                     ....   |||
                            |||                                        ||                 ....       |||
                            |||                                        ||              ....          |||
                            |||                                        ||          ....              |||
                            |||                                        ||       ....                 |||
                            |||                                        ||   ....                     |||
                            |||                                        ||....                        |||
                            |||                                        ||                            |||
                            |||                                        ||                            |||
                            |||                                        ||                            |||
                            |||                                        ||                            |||
                            |||                                        ||                            |||
                            |||                                        ||                            |||
                            |||                                        ||                            |||
                            |||                                        ||                            |||
                            ||+----------------------------------------++----------------------------+||
                            ||[h] call heads   [t] call tails   [n] change name   [q] leave           ||
                            ||You called tails. +1 coin. It rings on the table and stays put. Yours.  ||
                            ||The board has tossed 1 coins today and 0 yesterday.                     ||
                            ||Days the board has been keeping count: 1.                               ||
                            |+------------------------------------------------------------------------+|
                            +--------------------------------------------------------------------------+
```

### telnet 80x24, Nerd Font tier

```
--- door just opened
  ┌─────────────────────────────── russh-bbs ────────────────────────────────┐
  │Coin Toss                                                                 │
  │                                                                          │
  │┌ founder - day 20761 ───────────────────────────────────────────────────┐│
  ││Coins: 3   Tosses left today: 3   Streak: 0 (best 0)                    ││
  ││█████████████████████████2/5 tosses used today                          ││
  ││┌ This visit ────────────────────────────┐┌ Streak ────────────────────┐││
  │││Call heads or tails to start.           ││                            │││
  │││                                        │└────────────────────────────┘││
  │││                                        │┌ Purse ─────────────────────┐││
  │││                                        ││                            │││
  │││                                        ││                            │││
  │││                                        ││                            │││
  │││                                        ││                            │││
  │││                                        ││                            │││
  │││                                        ││                            │││
  │││                                        ││                            │││
  ││└────────────────────────────────────────┘└────────────────────────────┘││
  ││[h] call heads   [t] call tails   [n] change name   [q] leave           ││
  ││The board has tossed 2 coins today and 0 yesterday.                     ││
  ││Days the board has been keeping count: 1.                               ││
  │└────────────────────────────────────────────────────────────────────────┘│
  └──────────────────────────────────────────────────────────────────────────┘
  anyone on the network can read this session

--- after two tosses
  ┌─────────────────────────────── russh-bbs ────────────────────────────────┐
  │Coin Toss                                                                 │
  │                                                                          │
  │┌ founder - day 20761 ───────────────────────────────────────────────────┐│
  ││Coins: 3   Tosses left today: 1   Streak: 0 (best 1)                    ││
  ││█████████████████████████4/5 tosses used today ██████████▋              ││
  ││┌ This visit ────────────────────────────┐┌ Streak ────────────────────┐││
  │││#   Call   Result Coins                 ││█                           │││
  │││1   tails  won    4                     │└────────────────────────────┘││
  │││2   heads  lost   3                     │┌ Purse ─────────────────────┐││
  │││                                        ││                            │││
  │││                                        ││                            │││
  │││                                        ││       ⢀⣀⡠⠤⠔⠒⠊⠉⠒⠒⠤⠤⣀⣀       │││
  │││                                        ││⣀⡠⠤⠔⠒⠊⠉⠁             ⠉⠉⠒⠒⠤⠤⣀│││
  │││                                        ││                            │││
  │││                                        ││                            │││
  ││└────────────────────────────────────────┘└────────────────────────────┘││
  ││[h] call heads   [t] call tails   [n] change name   [q] leave           ││
  ││You called heads. -1 coin. It rolls off the edge of the table. Not yours││
  ││The board has tossed 4 coins today and 0 yesterday.                     ││
  ││Days the board has been keeping count: 1.                               ││
  │└────────────────────────────────────────────────────────────────────────┘│
  └──────────────────────────────────────────────────────────────────────────┘
  anyone on the network can read this session

--- after leaving the door
  ┌─────────────────────────────── russh-bbs ────────────────────────────────┐
  │Doors                                                                     │
  │                                                                          │
  │Pick a door: Up and Down, then Enter, or its number. Esc goes back.       │
  │                                                                          │
  │> [1] Coin Toss                                                           │
  │      Call it in the air. Five tosses a day.                              │
  │                                                                          │
  │Ctrl-C logs off from anywhere.                                            │
  │                                                                          │
  │                                                                          │
  │                                                                          │
  │                                                                          │
  │                                                                          │
  │                                                                          │
  │                                                                          │
  │                                                                          │
  │                                                                          │
  │                                                                          │
  │                                                                          │
  │                                                                          │
  │                                                                          │
  └──────────────────────────────────────────────────────────────────────────┘
  anyone on the network can read this session
```

