# Refactoring existing code (sub-skill)

Changing code that already works is a different job from writing new code. The standard in
SKILL.md still applies to the result; this file is about getting there without changing
what the code does. Read it for cleanups, modernisations, "remove the clones", and reviews
of someone else's diff.

Contents: Method · The moves, by smell · Worked examples · Reporting

## Method

1. **Equivalence first.** Before changing anything, write down what the old code does that
   the new code must still do: evaluation order, which awaits ran in sequence, when a lock
   is released, side effects, which errors escape and as what, distinct log events. A
   rewrite that turns sequential writes into `join!`, or merges two different "not found"
   branches, is a bug wearing a cleanup's clothes. When behaviour does change, verify it
   is harmless and say so in the report, or keep the original shape.
2. **Delete before you convert.** Dead branches, checks the type already proves, and arms
   subsumed by another arm come out first. The best refactor of redundant code is removal.
3. **One move per smell.** The table below pairs each shape with the move that replaces it.
   Apply the move, run the tests, then look for the next smell. Mixing several moves in one
   step hides which one broke the test.
4. **Keep the imperative form when it is honest.** A `for` loop that mutates locals, breaks
   early, or uses `?` on each step stays a loop. Never force an iterator chain that needs
   contortions; fix the reason for the contortion (usually a missing named helper).
5. **Let the compiler drive.** Change a type and follow the errors. A refactor that
   compiles because you added `.clone()` or `unwrap()` to make errors vanish has hidden the
   problem, not solved it.
6. **Verify with the project's tools.** `cargo fmt`, `clippy -D warnings` and the tests
   scoped to the touched code run green before the change is called safe. Add a test first
   when the behaviour being preserved has none.

## The moves, by smell

| Smell in the code | Move |
|---|---|
| `.unwrap()` / `.expect()` on input-dependent values | `?` with a typed error, `ok_or_else`, or `let else` (example 1) |
| `String` / `i64` ids passed around | Newtype minted at the boundary (example 2) |
| Struct with `status` and several `Option` fields | Enum whose variants carry their data (example 3) |
| `_ =>` over the project's own enum | List the variants so a new one breaks the match |
| `.clone()` added to satisfy the borrow checker | Borrow, restructure to end the borrow earlier, or take ownership at the call site (example 4) |
| `for` loop pushing into a `Vec` | `map`/`filter`/`collect`; keep the loop when it uses `?` or breaks early (example 5) |
| `std::fs` / `thread::sleep` / heavy CPU inside `async fn` | `tokio::fs`, `tokio::time::sleep`, or `spawn_blocking` (example 6) |
| Lock guard alive across `.await` | Narrow the scope, clone the data out, or move the state into one task behind a channel (example 7) |
| Detached `tokio::spawn` with no handle | `JoinSet` / `TaskTracker` plus a `CancellationToken` |
| `Box<dyn Error>` returned from a library | A `thiserror` enum with `#[from]` |
| `.map_err(|e| ...)` repeated at many call sites | Implement `From` once |
| `let _ = fallible();` or `.ok()` with no comment | Handle, propagate, or comment why it is safe to ignore |
| Library mock of the project's own trait | Hand-written fake behind `cfg(test)` (example 8) |
| `println!` / `dbg!` / `eprintln!` in library code | `tracing` event with a constant message and fields (example 9) |
| SQL or rules inside a front-end handler | Move rules to the core, SQL to the store (example 10) |
| `load_all().len() > 0` / filter-then-count | An `exists` query or `any` |
| `bool` parameter that switches behaviour | Two functions or an enum |
| A function with phases separated by comments | Named steps at one level of abstraction |
| `as` numeric cast that can truncate | `try_from` |
| `Vec<T>`/`String` parameter the function only reads | `&[T]` / `&str` |
| `lazy_static!` / `once_cell` | `std::sync::LazyLock` / `OnceLock` |
| Bare `#[allow(...)]` | `#[expect(..., reason = "...")]`, or fix the lint |

## Worked examples

### 1. Propagate instead of unwrap

Before:

```rust
fn load_key(path: &Path) -> HostKey {
    let bytes = std::fs::read(path).unwrap();
    HostKey::from_pem(&bytes).unwrap()
}
```

After:

```rust
fn load_key(path: &Path) -> Result<HostKey, KeyError> {
    let bytes = std::fs::read(path).map_err(|source| KeyError::Read { path: path.into(), source })?;
    HostKey::from_pem(&bytes).map_err(KeyError::Parse)
}
```

The caller (startup) now decides: fail the boot with a clear message. Behaviour changes
from panic to error return; say so.

### 2. A newtype instead of a raw id

Before: `fn redeem(code: String, user: i64, inviter: i64)`. After:
`fn redeem(code: &InviteCode, user: UserId, inviter: UserId)`. Swapping the last two
arguments no longer compiles. Mint `UserId` in the row mapper; the compiler lists every
place that still passes a raw `i64`.

### 3. An enum instead of a flag and options

Before: `struct Invite { status: Status, approved_by: Option<UserId>, redeemed_at:
Option<Timestamp> }`. Impossible combinations (redeemed but never approved) can be built.
After: the enum in `style.md`. Convert in the store's row mapper and let every `match`
become exhaustive. Delete the checks that guarded impossible states.

### 4. A clone that existed to quiet the borrow checker

Before:

```rust
let name = user.name.clone();
let greeting = format!("hello {name}");
log_login(&user);          // needed &user, so name was cloned earlier
```

After: `format!` borrows; reorder so the borrow ends before the call, and delete the clone.
If a clone is genuinely needed (a value moved into a task), move it at the spawn site and
leave a short comment on why.

### 5. Collect instead of push

Before:

```rust
let mut names = Vec::new();
for u in &users { if u.is_active() { names.push(u.name().to_owned()); } }
```

After: `let names: Vec<String> = users.iter().filter(|u| u.is_active()).map(|u| u.name().to_owned()).collect();`
If the loop body used `?`, an early `break` or two accumulators, it stays a loop.

### 6. Do not block the runtime

Before: `std::fs::read_to_string` and a password hash directly inside an `async fn`. After:
`tokio::fs::read_to_string(...).await` and
`tokio::task::spawn_blocking(move || hash(&password)).await??`. Check that the moved
closure owns its data (`move`), and keep the `JoinError` handled.

### 7. Do not hold a lock across an await

Before:

```rust
let mut guard = state.lock().unwrap();
guard.pending.push(item);
send(&guard.peer, msg).await?;   // guard still alive
```

After: take what you need, drop the guard, then await.

```rust
let peer = {
    let mut guard = state.lock().expect("state lock not poisoned");
    guard.pending.push(item);
    guard.peer.clone()
};
send(&peer, msg).await?;
```

When this pattern repeats, move the state into one task and talk to it over a channel.

### 8. A fake instead of a mock

Before: a mocking-library `MockInviteStore` with call-order expectations that break when an
internal call is reordered. After: a 15-line `FakeInviteStore { invites: Mutex<HashMap<...>> }`
in the crate's `testing` module, used by every service test. Assert on resulting state, not
on call counts.

### 9. A log event is a record with a message

Before: `println!("user {} redeemed {}", id, code);`. After:
`info!(user_id = %id, "invite redeemed")`, with the invite code deliberately not logged.
Check that nothing sensitive was in the old message; if it was, say so in the report.

### 10. Separate the concerns a handler had gathered

Before: one `async fn handle_post(...)` parses the line, runs SQL, applies the rate limit
and formats the reply. After: the front end parses and formats; `PostService::submit` holds
the rules and the rate-limit decision; `PostStore::insert` holds the SQL. Move one piece at
a time and run the tests after each. Verify error mapping did not change what the user
sees.

## Reporting

State what changed and what was proven equal, in plain sentences. When a conversion was
declined, give the specific blocker (sequential writes that must stay ordered, a lock that
must be held across the call, distinct log events), not a vague preference. When behaviour
differs in any observable way, that is the headline of the report, not a footnote. Name the
tests that cover the change.
