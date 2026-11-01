# Testing

Tests prove behaviour through the public surface. They do not prove wiring, and they do
not know which internal function was called. Sources: the Vitest guide and its v4/v5
migration notes, Node Best Practices section 4, Kent Beck's test shapes, Testcontainers
Node docs, MSW docs, Effective TypeScript item 55, the Google TypeScript Style Guide.

Contents: Version notes · Shape and naming · What to fake and what to run for real ·
Config · Databases · HTTP edges · Timers and clocks · Snapshots and tables · Type tests ·
The five outcomes

## Version notes

Check the installed Vitest major before writing config or mocks.

- Vitest 4 replaced `workspace` with `projects` in `vitest.config.ts`, removed
  `poolOptions`/`maxThreads` in favour of `maxWorkers`, dropped `coverage.all` (set
  `coverage.include` explicitly), made `mockReset` restore the original `vi.fn(impl)`
  implementation, and limited `vi.restoreAllMocks` to `vi.spyOn` spies.
- Vitest 5 requires Node 22.12+, makes `clearMocks: true` the default, fails a test on
  an unawaited async assertion (`expect(p).resolves` without `await`), fakes `Temporal`
  along with `Date`, removed `.sequential` (use `concurrent: false`), and no longer walks
  up directories to find config.
- `node:test` is a reasonable choice for a small dependency-free library or CLI. Vitest
  for application repos. One runner per package.

## Shape and naming

- `describe(unitName)` and `it('returns 404 when the order does not exist')`: the test
  name is the behaviour, in plain present tense, with no "should".
- Arrange, act, assert, separated by blank lines. One behaviour per test; several
  `expect`s are fine when they describe one outcome. No logic in tests: no loops, no
  conditionals, no helpers that compute the expected value the same way the code does.
- Tests sit beside the code: `orders.service.test.ts` next to `orders.service.ts`.
  `*.spec.ts` or `__tests__/` only when the repo already does that. Container-backed
  integration tests live under `test/integration/` as a separate Vitest project with a
  longer `testTimeout`.
- Fixture builders, one per table or aggregate, return a full valid value with
  overridable fields: `buildOrder({ status: 'cancelled' })`.
- A test that cannot fail is deleted. Coverage thresholds are a floor you ratchet up to
  catch untested files, never a target; chasing a number produces tests that assert
  nothing.

## What to fake and what to run for real

- Fakes over mocks. A fake is a small real implementation (`const clock = () => new
  Date('2026-01-01')`); a mock asserts on calls. Assert on outcomes, not on which
  internal function was called with what.
- Never `vi.mock` a module under `src/`. If a test needs to, the code under test imports
  a collaborator it should receive through its `deps` parameter. Fix the code, then pass
  a fake object. The composition root in `architecture.md` is what makes this possible.
- `vi.fn()` is for ports and adapters: the `OrderRepository` object handed to a factory,
  a `send` function on a fake queue client. Module mocking is reserved for third-party
  modules that cannot be injected, which is rare once there is a composition root.
- Repositories run against the real database in a container. Mocking the query builder
  tests nothing about the SQL.
- Controllers run through the real router with `supertest` (or Fastify's `inject`) and a
  fake service, so parsing, routing and error mapping are exercised.
- The network edge (OpenAI, S3, partner APIs) is mocked with MSW at the HTTP level,
  never by mocking your own client wrapper.

## Config

```ts
export default defineConfig({
  test: {
    environment: 'node',
    include: ['src/**/*.test.ts'],
    restoreMocks: true,
    coverage: {
      provider: 'v8',
      include: ['src/**'],
      thresholds: { lines: 80 },
    },
    projects: [
      { test: { name: 'unit', include: ['src/**/*.test.ts'] } },
      {
        test: {
          name: 'integration',
          include: ['test/integration/**/*.test.ts'],
          globalSetup: ['test/integration/global-setup.ts'],
          testTimeout: 30_000,
        },
      },
    ],
  },
});
```

`environment: 'node'` always; `jsdom` has no place in a backend repo.

## Databases

- Testcontainers starts Postgres once in `globalSetup`, runs migrations, and exports the
  URL through env. The container is never recreated per test.
- Vitest runs files in parallel, so each test file gets its own database or schema
  (testcontainers-node's per-file template-database helper does exactly this).
  Truncate between tests within a file.
- Insert through fixture builders, call the repository function, assert on the result
  and on the table state. Both halves matter: a function that returns the right value
  and writes the wrong row passes a result-only test.
- Transactions in tests mirror production: pass `trx` where the service would.

## HTTP edges

- MSW with `setupServer` from `msw/node`. `server.listen({ onUnhandledRequest: 'error'
  })` so a forgotten handler fails loudly instead of hitting the real service.
  `server.resetHandlers()` in `afterEach`, `server.close()` in `afterAll`.
- MSW intercepts global `fetch`, undici and `node:http`, so the code under test uses its
  real client with its real retry and timeout behaviour.

## Timers and clocks

- Prefer injecting `clock: () => Date` (or `now()`) over faking globals; the test then
  reads as data in, data out.
- When fake timers are unavoidable: `vi.useFakeTimers()` in `beforeEach`,
  `vi.useRealTimers()` in `afterEach`, and `await vi.advanceTimersByTimeAsync(1000)`.
  The synchronous variant leaves promises inside timers unresolved.

## Snapshots and tables

- `it.each` for tables, in the object form so failures read:
  `it.each([{ input: 'a', expected: 1 }])('maps $input to $expected', ...)`.
- Snapshots only for serialised output you own and review: an error payload shape,
  generated SQL, an OpenAPI document. Never for objects containing dates, ids or random
  values, never `toMatchSnapshot()` on a large DTO. `toMatchInlineSnapshot` for small
  values keeps the expectation visible in the test.

## Type tests

For exported types and type-level helpers: `expectTypeOf<Result>().toEqualTypeOf<...>()`
with `vitest --typecheck`. A type that silently widens is a bug no runtime test sees.

## The five outcomes

For a handler or service test, consider which of these the behaviour touches and assert
on each that applies (Node Best Practices 4.13):

1. the response or return value;
2. the state change (rows in the database);
3. the outgoing calls (what MSW received);
4. the messages queued;
5. the observability emitted (a log line or metric, through the injected logger).

A test that checks only the first can pass while the function does the wrong thing to
the world.
