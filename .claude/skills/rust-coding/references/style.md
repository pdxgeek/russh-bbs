# Style: types, functions, iterators, async and errors

The reasons behind the rules in SKILL.md, with the details that decide edge cases.
Sources: The Rust API Guidelines, the Rust Book and Reference, the Rust style guide
(`rustfmt` defaults), the Clippy lint documentation, Effective Rust (Drysdale), the Tokio
documentation, and the `tracing` and `thiserror` docs.

Contents: Version notes · Types · Type-level idioms · Absence and failure · Functions ·
Naming · Collections and iterators · Boundaries and validation · Errors · Async ·
Performance · Unsafe

## Version notes (check the installed toolchain before using an API)

- Read `rust-toolchain.toml` and `[workspace.package] edition`. This standard assumes
  stable and edition 2024. Features below that landed recently carry the version in
  brackets; if the pinned toolchain is older, use the alternative.
- `let ... else` (1.65), `async fn` in traits (1.75), `[workspace.lints]` (1.74),
  `#[expect(lint)]` (1.81), `std::sync::LazyLock` (1.80), let chains in `if`/`while`
  (stable in 1.88 for edition 2024 only).
- `thiserror` 2.x and `tokio` 1.x are the assumed majors; check `Cargo.lock`.
- Where the standard library covers a need, use it instead of a crate: `LazyLock` over
  `lazy_static`/`once_cell`, `std::iter` helpers over `itertools` for simple cases.

## Types

**Newtypes for ids and units.** A function taking `(UserId, InviteId)` cannot be called
with the arguments swapped; one taking `(i64, i64)` can. The cost is a few lines.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UserId(i64);

impl UserId {
    pub(crate) fn from_row(raw: i64) -> Self { Self(raw) }
    pub fn get(self) -> i64 { self.0 }
}
```

Mint ids only where they enter the system (a row mapper, a parser). Implement `TryFrom`
for values that need validating (`InviteCode::try_from(&str)`), so an invalid value never
exists.

**Enums carry their data.** Prefer

```rust
enum Invite {
    Pending { requested_at: Timestamp, note: Note },
    Approved { approved_by: UserId, expires_at: Timestamp },
    Redeemed { by: UserId, at: Timestamp },
}
```

over a struct with `status: Status` and four `Option` fields. Every consumer is forced to
handle each state, and impossible combinations cannot be built.

**No wildcard over your own enums.** `_ =>` hides the variant you add next month. List the
variants, or group them explicitly (`Pending | Approved => ...`). A wildcard is fine over
types you do not own (`std::io::ErrorKind`).

**Private fields, public behaviour.** A struct with an invariant (a non-empty name, a
sorted list) has private fields and a constructor returning `Result` or `Self`. Provide
accessors only for what callers need.

**Derives.** Derive what is correct, not what is possible. Do not derive `Default` when a
default value is not meaningful. Do not derive `Clone` on a type that owns a socket or a
secret just to satisfy the borrow checker. A type with secret fields gets a manual `Debug`
that prints `"[redacted]"`, and never `Display`.

**Lifetimes and ownership.** Prefer owned data in structs that cross task boundaries
(`'static` requirements in `tokio::spawn`). Use borrowed data in short-lived parsing and
view types. If a lifetime parameter spreads through five signatures, the data wants to be
owned or shared (`Arc<str>`, `Bytes`).

## Type-level idioms

- **Typestate** for protocols with a required order (a connection that must be
  authenticated before use): different types per state, methods consume `self` and return
  the next state. Use it where misuse is costly; otherwise a runtime enum is simpler.
- **`impl Trait`** in argument position for one-off generics, in return position for
  iterators and futures. Named generics when the type is used twice or needs a bound.
- **Sealed traits** for traits that must not be implemented outside the crate.
- **`#[must_use]`** on types and functions whose result is the point (builders, guards,
  `Result`-like wrappers).
- **`From`/`TryFrom`/`FromStr`** for conversions rather than `new_from_x` functions;
  `AsRef`/`Borrow` for generic borrowing.

## Absence and failure

`Option` is internal "absent". Do not mix `Option<Option<T>>`; model the three cases with
an enum. Database `NULL` maps to `Option` once, in the row mapper.

| Instead of | Write |
|---|---|
| `if x.is_some() { x.unwrap() }` | `if let Some(x) = x` or `let Some(x) = x else { return ... }` |
| `match o { Some(v) => v, None => return Err(E) }` | `let v = o.ok_or(E)?` or `ok_or_else` when `E` is built |
| `o.map(f).unwrap_or(false)` | `o.is_some_and(f)` |
| `v[0]`, `v[i]` | `v.first()`, `v.get(i)` with an explicit branch |
| `s[a..b]` on untrusted input | `s.get(a..b)`; slicing a `str` off a char boundary also panics |
| `x as u8` that can truncate | `u8::try_from(x)?` |
| `a + b` on untrusted sizes | `a.checked_add(b).ok_or(...)?` |

`unwrap_or_default()` and `unwrap_or_else` are fine; `unwrap_or(expensive())` evaluates
eagerly, so use `unwrap_or_else`. An `expect("...")` message states what must be true
("regex literal is valid"), not what failed.

## Functions

- One level of abstraction. `run_session` calls `authenticate`, `load_menu`, `render`; it
  does not also contain the SQL.
- Guard clauses and `?` flatten nesting. More than two levels of indentation is a signal.
- A function that both computes and performs I/O is two functions: a pure core that is
  trivially testable, and a thin shell that does the I/O.
- Parameters: take `&str` not `&String`, `&[T]` not `&Vec<T>`, `&Path` or
  `impl AsRef<Path>`. Take ownership only when the function stores or consumes the value,
  and then take it by value so the caller decides whether to clone.
- Return `impl Iterator<Item = T> + '_` to avoid allocating a `Vec` the caller may not
  need; return `Vec` when the caller needs ownership or random access.
- No output parameters. Return a struct or tuple of named fields.
- Builders for types with many optional fields; a plain struct literal with
  `..Default::default()` for small config.

## Naming

Follow the API Guidelines: `snake_case` for functions and modules, `UpperCamelCase` for
types and traits, `SCREAMING_SNAKE_CASE` for constants and statics. Getters are `name()`,
not `get_name()`. Conversions: `as_` is a cheap borrowed view, `to_` is an expensive or
owned conversion, `into_` consumes. Iterator methods are `iter`, `iter_mut`, `into_iter`.
Predicates read as questions (`is_expired`, `has_quota`). Traits are named for the
capability (`Store`, `Clock`, `FeedFetcher`) with no `I` prefix and no `Impl` suffix for a
sole implementation. Module names are nouns; avoid `util`, `helpers`, `common`, `misc`.

Types are not repeated in names that already carry the module path: `invite::Code`, not
`invite::InviteCode`, when callers write the path.

## Collections and iterators

- `Vec` by default; `VecDeque` for queues; `HashMap`/`HashSet` for lookup;
  `BTreeMap`/`BTreeSet` when order matters or output must be deterministic (tests,
  rendered lists). `HashMap` iteration order is random, so never depend on it.
- Pre-size with `Vec::with_capacity` when the length is known.
- Use iterator adaptors for transformation; use `for` for effects.

```rust
let names: Vec<&str> = users.iter().filter(|u| u.is_active()).map(User::name).collect();
let parsed: Result<Vec<Invite>, ParseError> = rows.iter().map(Invite::try_from).collect();
let (ok, bad): (Vec<_>, Vec<_>) = items.into_iter().partition(Item::is_valid);
```

- `entry` API for upsert-into-map; `fold`/`sum` for reductions; `chunks`/`windows` for
  slices; `zip`, `enumerate`, `rev` over index arithmetic.
- `sort_by_key` with `std::cmp::Reverse` for descending; `sort_unstable*` when stability
  is not needed; `min_by_key`/`max_by` instead of sorting to take one element. Floats need
  `total_cmp`.
- `.iter().cloned()`/`.copied()` only when the element is cheap; otherwise keep
  borrowing.
- Do not collect into a `Vec` just to iterate again. Do not call `.clone()` to satisfy the
  borrow checker without first trying to restructure; a clone that exists only to
  silence an error is a smell (see refactoring).

## Boundaries and validation

Parse, do not validate. At every trust boundary (SSH input, SFTP paths, feed bodies, mail,
config files, database rows) turn untyped input into a typed value once, and pass the typed
value inward.

```rust
#[derive(Deserialize)]
struct RawFeed { url: String, interval_s: u32 }

struct Feed { url: SafeUrl, interval: Duration }

impl TryFrom<RawFeed> for Feed {
    type Error = FeedError;
    fn try_from(raw: RawFeed) -> Result<Self, FeedError> {
        Ok(Self {
            url: SafeUrl::parse(&raw.url)?,
            interval: Duration::from_secs(u64::from(raw.interval_s).clamp(60, 86_400)),
        })
    }
}
```

`serde` with `#[serde(deny_unknown_fields)]` for strict inputs. Limits on length, depth and
count are part of validation, not an afterthought. Text from outside the program passes
through the shared sanitizer before it reaches storage or a terminal.

## Errors

- `thiserror` enums in libraries:

```rust
#[derive(Debug, thiserror::Error)]
pub enum RedeemError {
    #[error("invite code not found")]
    NotFound,
    #[error("invite expired")]
    Expired,
    #[error("store error")]
    Store(#[from] StoreError),
}
```

- Variants a caller branches on are separate; opaque internals collapse into one source
  variant. Do not return `Box<dyn Error>` from a library.
- `anyhow::Result` with `.context("loading host key")` in binaries and test helpers.
- `?` converts through `From`. If you reach for `.map_err(|e| ...)` in many places,
  define the `From`.
- Map errors to user-visible text in one function. The remote user sees a stable,
  non-revealing message; the log gets the full chain (`error = ?err` or `%err`).
- Never `let _ = result;` without a comment. Never `panic!` for a condition input can
  cause. `debug_assert!` documents internal invariants; `assert!` is for invariants whose
  violation means memory or data corruption.

## Async

- A future that is never awaited or spawned does nothing; the compiler warns, so keep
  warnings fatal.
- Blocking work: `tokio::task::spawn_blocking` for CPU-bound or synchronous library calls
  (password hashing, compression, `rusqlite`). A single blocking call on the runtime
  stalls every session.
- Locks: `tokio::sync::Mutex` only when the guard must live across an `.await`;
  otherwise `std::sync::Mutex` with the guard dropped before any await. Better still, give
  the state to one task and talk to it over a channel.
- Cancellation: dropping a future cancels it at its next await point. In `select!`, branches
  that lose are dropped; do not put non-cancellation-safe operations (partial reads into a
  buffer you then discard) in them.
- Timeouts everywhere there is I/O, with the value in config:
  `tokio::time::timeout(cfg.fetch_timeout, fetch(url)).await.map_err(|_| Error::Timeout)??`.
- Structured concurrency: `JoinSet` for a dynamic group, `TaskTracker` plus a
  `CancellationToken` for shutdown. Handle the `JoinError` (a panicked task) instead of
  ignoring it.
- `async fn` in traits is fine for internal traits; for traits used as `dyn`, use
  `Pin<Box<dyn Future>>` or the `async-trait` crate and say why.
- Channels: prefer bounded. An unbounded channel is a memory leak waiting for a slow
  consumer.

## Performance

- Measure before optimising. Say what you found and what you changed; if you spot an
  optimisation you did not make, name it in the report instead of rewriting silently.
- Avoid allocation in hot paths: borrow, reuse buffers, `Cow<'_, str>` when a value is
  usually unchanged, `Bytes` for shared byte buffers.
- A `clone()` of a `String` or `Vec` in a loop is the first thing to check. `Arc::clone`
  is cheap but not free; clone it at the task boundary, not per message.
- Batch database work: one query with `= ANY($1)` or a join instead of a query per row.
- `format!` in a hot log path allocates; use structured fields.
- Do not guess about `#[inline]`; leave it to the compiler unless a profile says
  otherwise.

## Unsafe

The workspace forbids `unsafe_code`. A crate that needs it (FFI, a measured hot path) opts
out in its own `Cargo.toml` with a comment, keeps `unsafe` blocks as small as possible,
puts a `// SAFETY:` comment above each stating the invariant relied on, and wraps them in a
safe API with tests (and Miri where the target supports it). If a safe crate already does
the job, use it.
