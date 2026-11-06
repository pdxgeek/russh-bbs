# H-4: loader fuzz and caps

Slice H-4, branch `hard/loader-fuzz`. Hardening of every loader that reads a sysop- or
file-supplied document. Tests only, plus the small fixes the tests found (D139).

## What is covered

Each row is a loader, the property-test file, and the inputs it gets. Every suite runs on
stable in `cargo test` (a few seconds each in a debug build). The inputs come from
`crates/server/src/fuzz.rs`: a valid document with one to five random edits (truncate, delete a
span, splice a hostile fragment or a reserved marker glyph, duplicate a line, repeat a span,
put a wrong-typed value after a `=`, swap lines, replace a character with any character, add an
unknown field or table, insert a huge string), random bytes, and structured garbage (deep arrays
and inline tables to 20 000 levels, a 6 000-part dotted key and table header, thousands of
`[[piece]]` or `[[menu]]` tables, thousands of keys, one key repeated, 150 000-character
strings and lines, NaN, infinity and out-of-range numbers).

| Loader | File | Entry point |
|---|---|---|
| Server config (`[board]`, `[widgets]`, `[blobs]`, every section) | `crates/server/src/config/fuzz_tests.rs` | `config::assemble`, `Loaded::load` |
| `menus.toml` and pages | `crates/server/src/ui/loader_fuzz.rs` | `sysop_menus::load` over a real directory |
| Theme files | `ui/loader_fuzz.rs` | `Theme::parse`, `ThemeSet::add_dir` |
| Theme packs (import) | `ui/loader_fuzz.rs`, `ui/theme/pack/fuzz_tests.rs` | `pack::parse`, `pack::install` |
| `.art.toml`, `.ans`, SAUCE, an art directory | `ui/loader_fuzz.rs` | `parse_art_toml`, `parse_ans`, `ArtSet::add_dir` |
| Door content TOML | `crates/doors/framework/src/content.rs` | `content::load` |
| PNG headers and bodies | `ui/image/fuzz_tests.rs` | `decode_png` |

## Properties checked

- **No panic** (a panic fails the property), on the default 2 MiB test-thread stack.
- **Caps hold**: bytes (config 256 KiB, theme 32 KiB, pack 512 KiB, art 64 KiB, `.ans` 64 KiB,
  menus 32 KiB and 600 lines, page 16 KiB and 200 lines, door table 512 KiB, PNG 256 KiB and
  1024 x 1024 and 1 000 000 pixels), grid size (132 x 50), file counts in a directory (themes
  32, art 64, packs 32), and time (every parse under 3 s in a debug build; the `.ans` worst
  cases at the byte cap: 64 KiB of newlines, home and clear, far cursor moves, 120-parameter
  SGR runs, an unterminated sequence).
- **A refusal falls back and says why once**: `menus.toml` or a page refused is the built-in
  menu with `problem()` set to one loggable line; a theme file refused leaves the default and
  the shipped themes unchanged (a sysop file cannot shadow a shipped one); an art directory of
  hostile files keeps the bundled art byte for byte and records one reason per refusal.
- **Reasons are loggable**: no control character, at most about 160 characters of the file's
  own text, on one line.
- **What loads is clean**: theme strings, titles, menu labels, page text, art cells (and their
  ASCII stand-ins, fallbacks and colours), and the config's board and widget text hold no
  control character, no reserved marker codepoint, no Nerd Font private-use glyph. Art cells
  have no attribute field, so art cannot produce the reserved style; themes are closed enums
  and colours, with no glyph field.
- **No secret in a message**: a canary password in `datasource.url`, under mutation, never
  appears in an error, a warning or `Debug`.
- **Pack import is all or nothing**: for each of the three files an install writes, a file or a
  directory already in the way makes the install fail with both directories as they were; a
  pack that parses installs whole and loads back with no art problem.

## Defects found and fixed in this slice

1. **A refusal repeated the file's text without a bound.** A 50 000 character key or value came
   back whole in the reason that is logged and shown to the sysop, in theme files, packs,
   `.art.toml`, `menus.toml`, pages, door content tables and the server config. The `.art.toml`
   reason was also several lines long, quoting the offending source line. Fixed with one cut
   (`ui::strings::reason`, escaped, one line, 160 characters; `ConfigError::invalid`; the door
   framework's `one_line`), applied where the file's text enters a reason. Regression tests:
   `a_refusal_never_repeats_a_long_value_or_key_whole` (ui),
   `a_config_refusal_never_repeats_a_long_value_or_key_whole`,
   `a_long_unknown_key_is_not_repeated_whole`. Verified to fail with the cut removed.
2. **The server config file was read with no byte cap** (`read_to_string`). A wrong path (a log,
   a device) was held whole. Now read through `take` at 256 KiB, and a non-UTF-8 file is a read
   error. Test: `an_oversized_config_file_is_refused_unread`.
3. **A door content validator could return unbounded problems.** `content::load` now keeps at
   most 20, one line each, with a count of the rest. Test:
   `a_validator_with_many_problems_is_summarised`.

## Not covered, and what was not verified

- No FIX rows: nothing found needed more than a small change.
- The reason is cut where the file's text is quoted. A new loader that quotes file text without
  `reason` would reintroduce the problem; the properties above catch it for the loaders listed.
- `Problem.reason` strings (art) and pack-list problems are shown on sysop screens; their
  drawing path was not re-walked here.
- Not run over a real client beyond the boot smoke below; the loaders are not reachable from a
  member session.
- Not run: the Postgres suite (nothing in the store changed).
