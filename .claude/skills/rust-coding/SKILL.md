---
name: rust-coding
description: How to write Rust in this codebase - the house standard for any Rust work on stable (edition 2024) with tokio, from a one-line fix to a new crate. Use it every time you write, change, review or test Rust - new modules, async handlers, trait definitions, storage code, TUI code, parsers, tests - even when the user only says "add a function", "fix this bug", "write the handler for X" or "clean this up" and never mentions Rust, style or quality. Also use it when reviewing a diff that shows unwrap() or expect(), clone() added to appease the borrow checker, String or i64 ids, a lock held across .await, blocking calls inside async fn, panics for expected failures, a wildcard `_ =>` arm over the project's own enum, println!, unsafe without a SAFETY comment, mocks of the project's own traits, or sleeps in tests. The refactoring sub-skill inside covers converting existing code safely.
---

# Writing Rust

This is how Rust gets written here: types that make illegal states unrepresentable,
small named functions, one home for every rule, boundaries that parse, errors that
travel to one place that can act on them, dependencies passed in through traits, and
tests that exercise behaviour. It follows the same principles as the Java and TypeScript
standards (`java-coding`, `typescript-coding`) so work moves between languages without
surprises. The standard is the same for a new crate and a two-line fix.

The body of this file is the standard in brief. The references carry the depth and the
reasons; open them when the task touches their area.

| When the task involves | Read |
|---|---|
| Any non-trivial type, function, iterator chain, async flow or error path | `references/style.md` |
| New crates or modules, workspace layout, trait wiring, config, lints, logging, security, shutdown | `references/architecture.md` |
| Writing or changing tests, deciding what to fake, databases, time, property tests | `references/testing.md` |
| Changing code that already works: cleanups, modernising, "remove the clones" | `references/refactoring.md` (the refactoring sub-skill) |

## How to work

**Before writing.** Read the surrounding module and one neighbour to learn the repo's
conventions: module layout, how dependencies arrive, how errors are typed, which logger
and test helpers are in use. The repo's convention wins over this document wherever they
differ, because consistency inside a codebase is worth more than any single rule. Check
`rust-toolchain.toml`, the edition and the locked majors of the crates you are about to
use before reaching for an API that changed between versions (see the version notes in
each reference).

**While writing.** Start from the types: the input, the output, the error cases, the
states. Then write the function so the compiler proves it. Keep each function at one level
of abstraction, name things for what they mean, and put each piece of knowledge in one
place. Parse at the boundary, trust inside. Let errors propagate to the one layer that can
act on them. Borrow by default and clone on purpose.

**The toolchain.** The pinned toolchain in `rust-toolchain.toml` is the source of truth;
rustup honours it. Never install Rust through a system package manager and never override
the toolchain in a script. Lints are configured once in `[workspace.lints]`, not with
scattered attributes.

**Before calling it done.** Run the project's checks:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```

A green build is not behaviour. Exercise user-visible changes through the real interface.
State what was verified and how; when something could not be verified, say so first.

**The operator's environment.** Do not touch the operator's data directory, host keys,
ports or databases. Tests bind port 0, use temporary directories and temporary databases,
and generate keys at test time. One-off scripts go in `/tmp/` with a header comment saying
what, why and when, and are deleted afterwards.

## The standard in brief

### Types prove the code

- Newtype every identifier and every unit (`UserId`, `InviteCode`, `Fingerprint`,
  `Bytes`). Raw `String`, `i64` and `Uuid` do not travel between modules. A newtype is
  minted only at the boundary and its constructor validates.
- Make illegal states unrepresentable. A state machine is an enum whose variants carry
  their own data, not a struct of `Option`s and a status flag. Match it exhaustively
  with no wildcard arm over the project's own enums, so adding a variant breaks every
  match that forgot it. `#[non_exhaustive]` only on enums that cross a published crate
  boundary.
- Structs have private fields and a constructor or builder that upholds the invariant;
  public fields only for plain data carriers with no invariant.
- Derive the obvious traits (`Debug`, `Clone`, `PartialEq`, `Eq`, `Hash`) when they are
  correct. Types that hold secrets implement `Debug` by hand or through a redacting
  wrapper and are never `Display`.
- No `unsafe` without a `// SAFETY:` comment stating the invariant and a test; the
  workspace sets `unsafe_code = "forbid"` and a crate opts out deliberately.
- Avoid stringly typed data and `Box<dyn Any>`. Prefer an enum or a generic parameter.

### Absence and failure

- `Option<T>` for absence, `Result<T, E>` for failure, never a sentinel value. Never
  `unwrap()` or `expect()` outside tests; an `expect` is allowed only on a proven
  invariant, with a message that states the invariant and not "failed to ...".
- `?` to propagate, `let ... else` for early exits, `ok_or_else` to turn `None` into a
  typed error, `map_or`/`is_some_and` over `match` ladders that only reduce to a bool.
- Indexing (`v[i]`, `&s[a..b]`) panics. Use `get`, `first`, `split_at_checked` and
  `str::get` when the index is not proven by construction.
- Arithmetic on untrusted numbers uses `checked_*`, `saturating_*` or `wrapping_*` on
  purpose. Casts with `as` that can truncate become `try_from`.
- Panics are for bugs, never for expected failures: bad input, missing file, closed
  connection, a timeout.

### Iterators, loops and match

- Iterator chains for uniform work over a sequence; a `for` loop when the body mutates
  locals, breaks early, or needs `?` on each step. Convert only when the chain reads
  better. Extract fat closures into named functions.
- Collect with the right collector (`collect::<Result<Vec<_>, _>>()`, `partition`,
  `fold` into a map); never `push` inside `for_each`. `any`/`all` over
  `filter().count() > 0`. `min_by_key`/`max_by` over sort-then-first.
- Push filtering, counting and existence into the database: an `exists` query, not
  load-all-then-check.
- `match` exhaustive; `if let` for one arm; `let else` for the early exit; `matches!` for a
  boolean check. Bind by reference unless ownership is needed.

### Functions and names

- One level of abstraction per function: it orchestrates named steps or it is a step. A
  body that needs section comments is several functions. Around thirty lines is the
  signal to look for the seam, and a file near three hundred lines is the signal to
  split the module.
- Guards first, early return, happy path at the shallowest indent. At most four
  parameters, then a struct. A `bool` parameter that switches behaviour is two functions
  or an enum.
- Take the most general borrow (`&str`, `&[T]`, `impl AsRef<Path>`, `impl IntoIterator`);
  return owned values or `impl Iterator`. Accept `impl Into<String>` only when the
  function stores the string.
- Pure functions in the middle, side effects at the edges. No hidden global state: no
  `static mut`, and `LazyLock`/`OnceLock` only for immutable data computed once.
- Rust naming: `snake_case` functions, `UpperCamelCase` types, no `get_` prefix on
  getters, `into_`/`as_`/`to_` per the standard conversion conventions, `is_`/`has_` for
  predicates, `try_` for fallible conversions. Collections plural; a map named by what it
  indexes (`policies_by_user`). No abbreviations beyond `id`, `url`, `db`, `ctx`.
- Comments say why, never what. `///` docs on every public item, with an example when it
  helps; `//!` at the top of each module.

### Errors

- Library crates: a `thiserror` enum per module or bounded context, with `#[from]` or
  `#[source]` so the cause chain survives. Binaries and tests: `anyhow` at the edges
  only, with `.context(...)` on I/O.
- Errors carry data a caller can branch on, not just a message. Messages are lowercase
  without trailing punctuation, per convention.
- Translate once per layer boundary. Domain errors cross layers as typed values and are
  mapped to what the user sees in exactly one place; no layer writes the user-facing text
  by hand. Never show internal error text, paths or SQL to a remote user.
- Never swallow. `let _ = fallible();` and `.ok()` need a comment saying why. Log with the
  error as a field, once, at the layer that handles it.

### Async

- No blocking inside `async fn`: no `std::fs`, `std::thread::sleep`, heavy CPU or
  synchronous crypto. Use `tokio::fs`, `tokio::time`, and `spawn_blocking` for the rest.
- Never hold a `std::sync::Mutex` guard, or any lock, across an `.await`. Prefer
  ownership transfer and channels (`mpsc`, `oneshot`, `watch`) over shared mutable state.
- Every external call has a timeout (`tokio::time::timeout`) and a size limit. Know which
  futures are cancellation-safe before using them in `select!`.
- Spawned tasks are owned: keep the `JoinHandle` or use a `JoinSet`/`TaskTracker`, and
  stop them with a `CancellationToken`. No detached `tokio::spawn` that can outlive its
  reason.
- Bounded channels and bounded concurrency (`Semaphore`, `buffer_unordered`) over
  user-sized input. Backpressure is a design decision, not an accident.

### Modules and wiring

- Follow the repo's layout. For a new one: a workspace of small crates with dependencies
  pointing toward the domain crate, and inside a crate, modules by feature.
- Dependency direction is one way: front end, service, store. The front end parses input
  and renders output. Services hold rules. The store holds SQL and returns mapped types.
  No terminal or network types below the front end, no SQL above the store.
- Dependencies arrive as constructor parameters typed as the narrowest trait the code
  uses. One composition root (`main`) builds the graph. No global singletons, no
  service locator. Generics for hot paths and internal seams; `Arc<dyn Trait>` where a
  runtime choice or a test fake is needed.
- Keep public APIs small: default to private, then `pub(crate)`, then `pub`.
- Config is read in one module, parsed into a typed struct with `serde` at startup, and
  passed down. A missing or invalid value fails the boot, not a request.
- Prefer a well-tested crate to hand-rolled code for logging, retries, parsing, hashing,
  time, argument parsing and cryptography. A new dependency needs a one-line
  justification and a clean `cargo deny check`.

### Logging

`tracing` with structured fields and a constant message naming the event:
`info!(user_id = %id, elapsed_ms, "invite redeemed")`. Spans carry request-scoped context.
Log once per event at the layer that owns it. Never log secrets, keys, tokens, message
bodies or mail. No `println!`, `eprintln!` or `dbg!` outside tests and binaries' own output.

### Tests

`#[cfg(test)] mod tests` beside the code for units; `tests/` for integration. Name tests
as the behaviour they prove. Arrange, act, assert. Fakes over mocks: pass a small fake
through the trait parameter; do not mock the project's own traits with a mocking library.
Time is injected, never slept. Databases in tests are real (temporary SQLite). Parsers and
sanitizers get property or fuzz tests. A test that cannot fail is deleted. Details in
`references/testing.md`.

## Refactoring existing code

Changing code that already works is its own discipline: equivalence first, delete before
converting, one move per smell, verified with the project's tests. The smell-to-move table
and worked examples live in `references/refactoring.md`. Read it whenever the task is a
cleanup, a modernisation or a review of someone else's diff.

## Reporting the work

State what was built or changed and what was verified, in plain sentences. Name the tests
that cover it. When a decision went against this standard because the repo does it
differently, say so in one line. When behaviour changed in any observable way, that is the
headline, not a footnote.
