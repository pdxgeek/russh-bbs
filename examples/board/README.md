# Example board kit

A small, original starter kit for a russh-bbs board: a menu, five sysop pages and a seventh style. Copy what you want, edit the words, and make it yours. Everything here is data your server already knows how to load; nothing is code.

```
examples/board/
  menus.toml                  the main menu, reordered, renamed and nested
  pages/                      five pages: rules, about, history, who's who, FAQ
    about.art.toml            small picture for the About page
    history.art.toml          small picture for the History page
  packs/tidepool.pack.toml    a theme pack: the "tidepool" style and its art
```

The words are a sample for a fictional board. Replace every `[bracketed]` placeholder and rewrite the pages in your own voice. Keep the page on the server's honest footing: the rules page says the server can read everything except PGP-encrypted mail and files members encrypt themselves, and yours should say the same.

## Install it

Your data directory is the folder with your SSH host key (`/data` in the container). Reading from the repository root:

```bash
cp examples/board/menus.toml  /data/menus.toml
cp -r examples/board/pages    /data/pages
mkdir -p /data/packs
cp examples/board/packs/tidepool.pack.toml /data/packs/
```

Then, signed in as the sysop:

1. **Sysop tools, `m`** (Menus and pages) shows the menu a member would see (`r` changes the role) and the pages (`t`). `l` reloads the files. People already signed in keep their menu; new sessions get the new one.
2. **Sysop tools, `p`** (Theme packs) lists `tidepool.pack.toml` as "ready: style tidepool". Press `i` on it to import. The style loads **when the server restarts**.
3. After the restart, pick it in Terminal settings, or make it the board's style with `board.style = "tidepool"`. A one-entry `board.styles` locks it for everyone.

If a file is wrong, the server keeps the built-in menu (or skips the pack) and tells you the one reason. It never stops the board from starting.

## What the menu file shows

`menus.toml` is a worked example of every move the file allows:

- **Reordering.** The Notice board and Message boards come before Mail; the built-in items you do not list (Members, Community, Invites, Settings, Sysop tools, Log off) keep their place at the end.
- **Renaming and re-keying.** Mail becomes "Post office" on `p`; Forums becomes "Message boards".
- **Nesting.** Front Hall, then Notice board, then Board lore: three levels. Groups show only when something inside them is visible.
- **Pages.** Five pages are linked from groups. Pages are content: the server sanitizes every line.
- **Hiding.** A commented-out block shows how to hide a built-in item for everyone.

You cannot add an action, a target or a permission. A built-in item you move into a group keeps the permission of the menu it came from, so moving things can only narrow who sees them. Caps: 32 KiB, 24 items a menu, 8 groups, 16 pages, 32-character labels.

## What a page looks like

`pages/<id>.toml`: a `title` (32 characters), optional `art`, and `text`.

```
# Heading        a heading
Plain lines      wrap to the screen
- a bullet       bullet
> a quotation    quotation
---              a rule
```

Caps: 16 KiB, 200 lines, 160 characters a line. The `art` is a file in `pages/` (an `.art.toml` whose first `area` piece is used, or a `.ans`).

## The style and its art

The pack is a style exported from Paper with Sysop tools and edited: a rock pool at dusk, with a login banner, a menu header, an area header and a logoff picture. Open it in an editor:

- `theme = '''...'''` is a theme file (colours for 24 roles, borders, title and header treatment). A pair of colours that is hard to read is refused with the reason.
- `[[art]]` holds an `.art.toml`: `rows` is the picture, `fg` and `bg` hold one colour letter per cell (`.` is the style's own; `k b g c r m y w` and capitals for bright), `ascii` is the version for the plain-ASCII terminal tier, and `fallback` is the one line shown when the window is too narrow.
- Rename it by changing `name` in the theme and the art file (and `style` in the art). A style name that is already installed is refused.

Art is cells, never bytes: a piece cannot set bold or reverse video, so it cannot imitate the security marker. The marker glyphs and the private-use range are neutralized on the way in, and art never covers the status row.

## Make your own art

Credit yourself in `credit` (up to 80 characters). Keep art original: do not copy pieces from other boards, art packs or groups. The caps are 80 columns by 25 rows for headers (an area header should be three rows or fewer to leave room at 80x24), and 132 by 50 for login and logoff pieces. Give every piece `ascii` rows and a `fallback`.

You can check a set of files without a server: put them in a scratch data directory, start the board, and open **Sysop tools, `t`** (Theme preview) to see each style, layout, size and tier; refused files are listed there with their reasons.

## Not covered here

- A separate header for each area (forums, mail, chat, files, doors): the board draws one `area` header per style on forums, mail and chat. See `FIX-ART-1` in `docs/coordination.md`.
- Telnet, other terminals and a real Nerd Font were not checked for this kit; see `docs/qa/art-1-walk.md`.
