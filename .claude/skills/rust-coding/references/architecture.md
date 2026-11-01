# Architecture: layout, wiring, config, tooling, logging, operations

Where code lives, how it is wired, and the project-level settings that keep it honest.
Sources: The Cargo Book, the Rust API Guidelines, Effective Rust, the Tokio documentation
(graceful shutdown), the `tracing` docs, `cargo-deny`, and the SQLx documentation.

Contents: Project layout · Layers · Dependency wiring · Config · Cargo and lints · Lint and
format · Database access · Logging · Security basics · Process lifecycle

## Project layout

A Cargo workspace of small crates. The domain crate depends on nothing concrete; everything
else depends on it.

```
Cargo.toml                 [workspace], [workspace.package], [workspace.lints], [workspace.dependencies]
rust-toolchain.toml        pinned channel, components (rustfmt, clippy)
deny.toml                  licenses, advisories, bans
crates/
  core/                    domain types, traits, rules (no I/O)
  store/                   implements core's storage traits
  server/                  binary: front ends, composition root
```

Inside a crate, modules by feature, not by kind:

```
crates/core/src/
  lib.rs                   re-exports the public surface, nothing else
  invite/                  mod.rs, state.rs, code.rs, rules.rs, tests.rs
  mail/
  sanitize.rs
```

Rules:
- `lib.rs` and `mod.rs` hold declarations and re-exports, not logic.
- One concept per file; split a file before it becomes a monolith.
- Dependencies point toward `core`. A cycle between crates means a trait belongs in `core`.
- Versions of shared dependencies are declared once in `[workspace.dependencies]` and
  inherited (`foo = { workspace = true }`).
- A binary crate stays thin: parse config, build the graph, run. Logic lives in libraries
  so it can be tested.

## Layers

```
front end (SSH, SFTP, GraphQL)  ->  service / rules (core)  ->  store (SQL, files)
```

- The front end parses input, calls the core, and renders results. It owns terminal types,
  protocol types and the mapping of errors to user-visible text. No rules.
- The core holds rules and owns the transaction boundary (through a store trait). It does
  not know about terminals, sockets, SQLite or the filesystem.
- The store holds SQL and file access, returns domain types mapped once at the edge, and
  implements core traits. No rules.
- A rule that exists in two front ends is a bug; move it down.

## Dependency wiring

Dependencies are traits passed into constructors.

```rust
pub trait Clock: Send + Sync { fn now(&self) -> Timestamp; }

pub trait InviteStore: Send + Sync {
    async fn find(&self, code: &InviteCode) -> Result<Option<Invite>, StoreError>;
    async fn mark_redeemed(&self, code: &InviteCode, by: UserId, at: Timestamp)
        -> Result<(), StoreError>;
}

pub struct InviteService<S, C> { store: S, clock: C }

impl<S: InviteStore, C: Clock> InviteService<S, C> {
    pub fn new(store: S, clock: C) -> Self { Self { store, clock } }
    pub async fn redeem(&self, code: &InviteCode, by: UserId) -> Result<(), RedeemError> { /* ... */ }
}
```

- Each service takes the narrowest trait it uses, not a whole "database" object.
- Generics for internal seams and hot paths (static dispatch, no allocation); `Arc<dyn
  Trait>` where the implementation is chosen at runtime or the type would otherwise
  infect many signatures. Pick one per seam and keep it.
- The composition root in `main` builds the concrete graph and is the only place that
  names `SqliteStore`. Tests build the same services with fakes.
- No global singletons (`static DB: ...`), no service locators, no
  `lazy_static`-initialised connections. `LazyLock` is for immutable, computed-once data
  (a compiled regex, a lookup table).
- Traits live with the consumer (in `core`), not with the implementation.

## Config

- One module reads configuration (file, environment, flags), deserialises it with `serde`
  into a typed struct, validates it, and returns it. The program fails to start on a bad
  value; a request never discovers it.
- Pass the typed config, or the slice a component needs, down as parameters. Do not read
  the environment from inside a service.
- Secrets come from the environment or a file with restricted permissions and are never
  printed, logged, or put in `Debug` output.
- Durations, sizes and limits are typed (`Duration`, a `ByteSize` newtype), not bare
  integers whose unit is a comment.

## Cargo and lints

Declare the lint policy once for the workspace:

```toml
[workspace.lints.rust]
unsafe_code = "forbid"
missing_docs = "warn"
unreachable_pub = "warn"

[workspace.lints.clippy]
all = { level = "warn", priority = -1 }
pedantic = { level = "warn", priority = -1 }
unwrap_used = "deny"
expect_used = "warn"
panic = "deny"
dbg_macro = "deny"
print_stdout = "warn"
todo = "warn"
# pedantic lints the team decided not to follow, each with a reason:
module_name_repetitions = "allow"
missing_errors_doc = "allow"
```

Each crate opts in with `[lints] workspace = true`. CI runs clippy with `-D warnings`, so
every warning is an error. Silencing a lint uses `#[expect(lint, reason = "...")]`, which
fails when the lint stops firing, never a bare `#[allow]`. Tests may allow
`unwrap_used`/`expect_used` locally in the test module.

Other Cargo settings:
- `rust-toolchain.toml` pins the channel; `rust-version` (MSRV) is set in
  `[workspace.package]` if the crate is published.
- Features are additive and documented; no feature changes behaviour incompatibly.
- `Cargo.lock` is committed for binaries.
- Release profile settings (`lto`, `codegen-units`, `panic`) are decided deliberately, not
  copied. Keep `panic = "unwind"` if tasks should survive a panic in one session.

## Lint and format

- `cargo fmt --all` with the default `rustfmt` style, no per-project reformatting. Imports
  are grouped and sorted by the tool.
- `cargo clippy --workspace --all-targets -- -D warnings`.
- `cargo deny check` for licenses, advisories, duplicate versions and banned crates.
- `cargo doc --no-deps` with `RUSTDOCFLAGS="-D warnings"` keeps doc links honest.
- `cargo machete` or `cargo udeps` occasionally for unused dependencies.

## Database access

- Use `sqlx` with compile-time checked queries (`query!`/`query_as!`) so row types come from
  the schema, and commit the offline query data (`.sqlx/`, produced by `cargo sqlx
  prepare`). CI builds with `SQLX_OFFLINE=true` and fails if the data is stale.
- Migrations are forward-only, versioned files that are never edited once merged.
- Map rows to domain types in the store; the core never sees a row type.
- Transactions are opened by the store method that implements a use case, not by the front
  end. Hold a transaction for as short a time as possible and never across a network call.
- Batch with joins or `= ANY($1)`; no query per element of a loop.
- Keep SQLite in WAL mode, set a busy timeout, and size the pool deliberately.

## Logging

- `tracing` with `tracing-subscriber`; one subscriber installed in `main`.
- Constant message naming the event, values as fields:
  `info!(user_id = %user, elapsed_ms = took.as_millis(), "invite redeemed")`.
- Spans for request or session context (`#[instrument(skip(self, secret))]`); skip
  arguments that are large or sensitive.
- Levels: `error` means someone must act, `warn` means handled but abnormal, `info` is a
  business lifecycle event (one or two per session), `debug`/`trace` are for development.
- Log once, at the layer that handles the failure. Do not log and also return the same
  error.
- Never log secrets, keys, tokens, message bodies, mail content or invite codes. Types
  holding secrets have redacting `Debug`.

## Security basics

- Input is hostile: limit sizes, counts, depth and time at every boundary. Put limits in
  config.
- Text from users, feeds, mail and files passes through the shared sanitizer before being
  stored or shown.
- Fetching a URL a user supplied goes through the SSRF guard (see the project threat
  model); do not call an HTTP client directly on such a URL.
- Randomness for secrets comes from the OS CSPRNG (`rand::rngs::OsRng` or `getrandom`).
  Compare secrets in constant time (`subtle`). Zero key material with `zeroize`.
- Use audited crates for cryptography; do not implement primitives or protocols.
- `cargo deny` and a dependency review for anything new; prefer fewer, well-maintained
  dependencies.
- File paths from users never reach the filesystem; they are mapped to IDs first.

## Process lifecycle

- `#[tokio::main]` only in the binary. Build the runtime deliberately if worker threads or
  blocking-pool limits matter.
- Graceful shutdown: catch SIGINT/SIGTERM, cancel a root `CancellationToken`, stop
  accepting new work, let in-flight work finish within a deadline, flush, then exit.
  Tasks watch the token in their `select!`.
- A panic in a task is a bug: log it, fail the task, keep the process up if sessions are
  independent. Install a panic hook that logs through `tracing`.
- Health: the process fails fast at startup on bad config, a missing migration, or an
  unreadable host key. It does not start half-broken.
- State on disk (database, host keys, uploads) lives under one configured data directory
  with restrictive permissions. Back up and restore are tested procedures, not hopes.
