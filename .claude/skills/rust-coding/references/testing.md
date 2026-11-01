# Testing

What to test, how to shape it, and what to fake. Sources: the Rust Book testing chapters,
`cargo-nextest`, `proptest`, `insta`, Tokio's testing utilities, and the Java and
TypeScript testing references in this repository's skills.

Contents: Version notes · Shape and naming · What to fake and what to run for real · Time
and clocks · Databases · Network and ports · Async tests · Property and fuzz tests ·
Snapshots and tables · Security tests · The five outcomes · Reporting

## Version notes

- `cargo test` is the baseline; `cargo nextest run` is faster and isolates tests per
  process. Use whichever the repo's CI uses; doctests still run under `cargo test --doc`.
- `tokio::test` needs the `macros` and `rt` features; `start_paused = true` needs
  `test-util`.
- Check `Cargo.lock` for the majors of `proptest`, `insta` and `tokio` before using
  version-specific APIs.

## Shape and naming

- Unit tests live beside the code in `#[cfg(test)] mod tests`. Integration tests live in
  `tests/` and use only the public API.
- Name a test as the behaviour it proves in plain present tense:
  `redeeming_an_expired_invite_is_rejected`, not `test_redeem_2`.
- Arrange, act, assert, in that order, with a blank line between. One behaviour per test.
- Table-driven cases for input/expected pairs: a `const` array of cases and a loop with the
  case in the failure message, or a macro. Do not copy-paste a test body.
- Assert on values, not on `is_ok()`. Use `assert_eq!` with expected on the right-hand
  convention the repo uses, and `matches!` for enum variants.
- Tests do not depend on order, wall-clock time, the network, the home directory, or an
  environment variable they did not set.
- A test that cannot fail is deleted. Tests that are `#[ignore]`d carry a reason and an
  owner.

## What to fake and what to run for real

- **Fake your own traits by hand.** A fake is a small struct implementing the trait with an
  in-memory `Vec` or `HashMap`. It lives in a `testing` module next to the trait, behind
  `#[cfg(any(test, feature = "testing"))]`. Hand-written fakes read like documentation and
  do not break when an internal call order changes.
- Do not mock the project's own traits with a mocking library, and do not assert on the
  number of calls to a collaborator unless the call is the behaviour (sending a message).
- Mock only at the edge you do not own: the HTTP server of a third party (a local test
  server on port 0), the SMTP relay, the clock, the random source.
- Run real code for everything inside the process: parsers, sanitizer, rules, the store
  against a real temporary SQLite database.

## Time and clocks

- Inject a `Clock` trait (see architecture). Production uses the system clock; tests use a
  fake whose time they advance.
- For tokio timers use `#[tokio::test(start_paused = true)]` and
  `tokio::time::advance(...)`. Never `sleep` in a test to wait for something; wait on the
  event (a channel, a `Notify`, a join handle) with a timeout.
- Fix randomness with a seeded generator injected into the code under test.

## Databases

- Repositories are tested against a real SQLite database: in-memory for speed, a temp file
  when WAL or file behaviour matters. Run the real migrations before each test.
- Each test gets its own database; no shared state, no cleanup order.
- Test the queries that carry rules: uniqueness, constraints, ordering, the `exists` and
  batch queries, and migrations applying cleanly from empty.
- If the repo uses `sqlx` offline data, a test run must not require a developer's local
  database.

## Network and ports

- Bind port 0 and read the assigned address; never a fixed port. Tests that open an SSH
  server do so on `127.0.0.1:0` with a host key generated in the test.
- Drive the server with a real client library or the `ssh` binary where the behaviour is
  about the protocol; keep these few and fast.
- Third-party HTTP is faked with a local server (`wiremock` or an `axum` handler) that fails
  on unexpected requests.
- Never touch the operator's data directory, real host keys, or real ports.

## Async tests

- `#[tokio::test]` for async tests; `flavor = "multi_thread"` only when the behaviour needs
  it.
- Wrap anything that could hang in `tokio::time::timeout` so a regression fails instead of
  stalling CI.
- Assert that spawned tasks finished and did not panic: await the `JoinHandle`.

## Property and fuzz tests

- Use `proptest` for functions with a large input space and a stated invariant: the
  sanitizer never outputs a control character, parsing then printing round-trips, a limit
  is never exceeded.
- Parsers and the sanitizer also get a `cargo-fuzz` target; commit the corpus of
  interesting inputs and any crash reproducer as a regression test.
- Prefer a few strong properties to many weak ones, and shrink failures to a minimal case
  recorded as a plain unit test.

## Snapshots and tables

- `insta` snapshots suit rendered screens and larger structured output. Review snapshot
  diffs like code; never accept a snapshot blindly.
- Keep snapshots small, deterministic (fixed sizes, fixed time, `BTreeMap`), and free of
  secrets and machine-specific paths.

## Security tests

Tests prove the security rules in the project's threat model, not just the happy path:

- Untrusted text cannot inject terminal escapes or private-use icon characters.
- A forbidden URL (loopback, private, metadata address, bad scheme, redirect to private) is
  refused.
- An unauthenticated or unknown identity can reach only what the waiting room allows.
- Content the server can read is never presented as private.
- Limits (size, count, rate) are enforced and fail closed.

## The five outcomes

For a service or handler test, consider which of these the behaviour touches and assert on
each that applies:

1. the return value or the rendered output;
2. the state change (rows, files);
3. the outgoing calls (what the faked remote received);
4. the messages queued or sent;
5. the observability emitted (a log event or metric, through the injected subscriber).

A test that checks only the first can pass while the function does the wrong thing to the
world.

## Reporting

Say which tests you ran and the result, which you could not run and why, and what the
tests do not cover. A new behaviour with no test is reported as unverified. Flaky tests are
fixed or removed, never retried into passing.
