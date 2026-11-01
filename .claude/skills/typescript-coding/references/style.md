# Style: types, functions, async and errors

The reasons behind the rules in SKILL.md, with the details that decide edge cases.
Sources: Effective TypeScript 2nd ed. (Vanderkam, 2024; item numbers below), the
TypeScript handbook and 5.x/6.0/7.0 release notes, the Google TypeScript Style Guide,
typescript-eslint `strict-type-checked` and `stylistic-type-checked`, the Node.js API
docs, the Node Best Practices repo (goldbergyoni), Clean Code chapter 3 for function
shape, Zod 4 docs, RFC 9457.

Contents: Version notes · Types · Type-level idioms · Nullability · Functions · Naming ·
Collections · Boundaries and validation · Errors · Async · Performance on the event loop

## Version notes (check the installed major before using an API)

- TypeScript 6.0 (March 2026) changed defaults: `strict: true`, `module: esnext`,
  `target: es2025`, `types: []`. TypeScript 7.0 (July 2026, the Go compiler) turned the
  6.0 deprecations into hard errors: `moduleResolution: node`/`node10`, `baseUrl`,
  `target: es5`, `esModuleInterop: false`, `outFile`. 7.0 has no programmatic API yet,
  so typed lint still runs on the 6.x package (`typescript@npm:@typescript/typescript6`).
- Node runs `.ts` files directly by stripping types; stable since 24.12 / 25.2 and the
  default since 22.18 / 23.6. The `--experimental-transform-types` flag that compiled
  enums and parameter properties was removed in Node 26, so non-erasable syntax does not
  run on current Node without a build step. Node never type-checks: CI runs `tsc --noEmit`.
- Node 22 is in maintenance, 24 is active LTS until October 2026, 26 became LTS on
  28 October 2026 (Temporal on by default). Production uses an LTS line.
- Zod 4 moved formats to the top level (`z.email()`, `z.uuid()`, `z.iso.datetime()`),
  unified error customisation under one `error` option, and replaced `.strict()` and
  `.passthrough()` with `z.strictObject()` and `z.looseObject()`. See Boundaries.
- Express 5 forwards async rejections to the error middleware natively. See Errors.
- Vitest 4 and 5 changed mocking defaults and config keys. See `testing.md`.

## Types

**No escape hatches (items 5, 9, 38 to 46)**
- `any` turns the checker off for everything it touches and spreads through inference.
  `unknown` for values not yet examined; narrow with `typeof`, `instanceof`, `in`, a
  user-defined type guard, or a schema parse. Narrow at the boundary once; inside,
  everything is typed. `Record<string, unknown>` over `object` or `{}`.
- The non-null assertion `!` is an unverified claim. Narrow (`if (!x) throw new
  NotFoundError(...)`) or make the type honest (`T | undefined`). `!` on `Map.get` is
  the commonest offender; `??` or a guard fixes it.
- `as` casts are claims too. Allowed: `as const`, and a cast at a verified boundary
  with a one-line comment saying why the checker cannot see it. Never `as unknown as T`.
  Prefer `satisfies` or an annotation when the goal is to check a literal.
- `@ts-expect-error` with a reason and a link, for library bugs only; it fails when the
  error disappears, which is what you want. `@ts-ignore` never.
- `Object.keys(o) as (keyof T)[]` is wrong: keys are `string` because an object may
  carry more properties than its type (item 60).

**Shapes**
- `interface` for object shapes, `type` for unions, intersections and aliases. Be
  consistent within a module.
- Unions of objects over objects with unions (item 34): `{ kind: 'card'; last4 } |
  { kind: 'bank'; iban }` makes the impossible state unrepresentable; `{ kind; last4?;
  iban? }` invites it. Limit optional properties (item 37). Do not put `| undefined`
  inside a type alias; let the field be optional.
- No `enum`. `erasableSyntaxOnly` rejects it and Node cannot strip it. Use
  `const Status = { Active: 'active', Closed: 'closed' } as const;
  type Status = (typeof Status)[keyof typeof Status];` when runtime iteration is needed
  (`Object.values(Status)` feeds `z.enum`), or a plain string-literal union when it is not.
  `const enum` is banned for the same reason plus isolated-modules breakage.
- `readonly` by default on parameters and data: `readonly string[]`, `Readonly<Order>`,
  `ReadonlyMap`. A readonly array accepts a mutable one, so it costs callers nothing
  and catches accidental mutation of shared input (item 14).
- Accept the narrowest type the function reads (`Pick<Property, 'id' | 'companyId'>`);
  return the most precise type you can. Never return `T | null | undefined` when
  `T | undefined` will do.
- Branded ids: `type UserId = string & { readonly __brand: unique symbol }` or
  `z.string().uuid().brand<'UserId'>()`. Mint them in the parser or repository, never
  with `as` in business code. Zero runtime cost, and `OrderId` stops flowing into a
  `UserId` parameter.

**Inference**
- Let locals infer. Annotate every exported function's parameters and return type: the
  return annotation documents intent, catches drift when a branch is added, and speeds
  declaration emit.
- `z.infer<typeof Schema>` is the one source of truth for a boundary type. Never write
  the interface and the schema separately; they drift.
- Object literals passed to a typed parameter or annotated with `: Foo` get excess
  property checking; literals assigned to an inferred `const` and later passed do not.
  Annotate or `satisfies` at the literal when the shape matters.

## Type-level idioms

- `satisfies` for config and lookup objects: `const routes = { ... } satisfies
  Record<string, Handler>` validates the shape and keeps the narrow keys, so
  `routes.home` still autocompletes. Annotation when the wide type is wanted; `as` never.
- `const` type parameters (TS 5.0): `function defineRoutes<const T extends readonly
  Route[]>(routes: T)` keeps literal types without forcing callers to write `as const`.
- Inferred type predicates (TS 5.5): `arr.filter((x) => x !== undefined)` narrows on
  its own. Stop writing `filter(Boolean) as T[]`.
- Exhaustiveness: `switch (x.kind) { ... default: x satisfies never }` or
  `const _exhaustive: never = x`. With `switch-exhaustiveness-check` set to
  `considerDefaultExhaustiveForUnions: false`, a `default` cannot hide a missing case.
- Generics only when a type parameter relates two or more positions (item 51). `<T>(x:
  T): void` is `(x: unknown): void`. `NoInfer<T>` (TS 5.4) pins inference to one
  argument when a second would widen the union.
- Overloads only when the return type depends on the argument and a conditional type
  would be unreadable (item 52). Never overload to express an optional argument.
- Template literal types for small closed vocabularies (event names, `${Env}_${Region}`),
  never for parsing arbitrary strings; that is a validator's job.
- `import type { X }` or inline `import { type X }`, enforced by `verbatimModuleSyntax`
  and `consistent-type-imports`. Re-export types with `export type { X } from`.

## Nullability

- `undefined` is the internal "absent". `null` enters from the database and JSON and is
  mapped to `undefined` or a default once, in the row mapper or schema (item 33 pushes
  null to the perimeter). Internal code never checks both.
- `x == null` is the one sanctioned loose equality, because it covers both. Prefer `??`
  and `?.` when the shape allows.
- `a?.b?.c ?? fallback` replaces a guard ladder when every missing link means the same
  thing. A missing link that means something different (it logs, it returns a different
  status) keeps its guard.
- `??` for defaults; `||` only when `0`, `''` and `false` are genuinely meant to fall
  through. `??=` for lazy initialisation. `prefer-nullish-coalescing` enforces it.
- `noUncheckedIndexedAccess` makes `arr[0]` and `record[key]` `T | undefined`, which is
  the truth. Handle with `.at()`, destructuring defaults, or a guard.
- Optional parameters last, with defaults in the signature. An optional field means
  "may be absent"; it is not a way to say "may be null".

## Functions

**Size and shape**
- One level of abstraction per function. An orchestrator calls named steps; a step does
  one thing. Mixing them is what makes a function need section comments.
- Around twenty lines is a signal, not a rule to game: past it, look for the phase
  boundary.
- Guards first, early return, happy path at the shallowest indent. Nesting past two
  levels is a function waiting to be extracted.
- At most three positional parameters; past that, an options object with named fields.
  A boolean parameter that switches behaviour is two functions: `createProperty(input,
  true)` tells the reader nothing; `createMultiUnitProperty(input)` does.
- Do not mutate arguments. Return new values with `toSorted`, `toReversed`, `with`,
  spread.
- Pure functions in the middle, side effects at the edges. A function that computes and
  also writes to the database is two functions.
- One shape of return, never `Property | null | 'not-allowed'`. When the caller must
  branch on an expected outcome, return a result union with a discriminant; when it
  cannot recover, throw a typed error. Never both from one function.
- Do not start async work in constructors or at module top level, apart from config
  parsing. It is untestable, its rejections are uncatchable, and top-level `await`
  makes import order into execution order.

**Classes**
- Factory functions returning an object of functions are the default: no `this`, no
  decorators, no parameter properties, trivial to fake. Classes are fine for errors
  (`instanceof`) and for real state plus behaviour.
- No class parameter properties (`constructor(private x: T)`): `erasableSyntaxOnly`
  rejects them. Declare the field and assign it.

## Naming

- Functions are verbs or verb phrases that state the effect: `loadActivePrompt`,
  `markOldCriteriaRejected`, `toPropertyRow`. `get` for cheap synchronous reads, `load`
  or `fetch` for I/O, `to`/`from` for mappers, `build` for pure construction, `ensure`
  for idempotent setup.
- Booleans are predicates: `isMultiUnit`, `hasActivePolicy`, `canEdit`, `shouldRetry`.
- Collections are plural nouns; a `Map` is named by what it indexes
  (`policiesByPropertyId`).
- Types and classes are PascalCase nouns. No `I` prefix, no `Impl` suffix, no `Data`,
  `Info`, `Manager` filler.
- `UPPER_SNAKE` only for true module-level constants; a `const` local is camelCase.
- No abbreviations beyond `id`, `url`, `db`, `req`, `res`, `ctx`. `prop`, `cnt`, `tmp`,
  `val`, `arr` are not names.
- Files: follow the repo. Where there is none, kebab-case with a role suffix
  (`user.repository.ts`, `user.service.test.ts`).
- A name that needs a comment is the wrong name. A comment says why, never what.

## Collections

- `map`, `filter`, `flatMap`, `some`, `every`, `find` for uniform work. `for...of` when
  the body mutates locals, breaks early, or awaits sequentially.
- `forEach` that pushes into an array is a `map` or `filter`. `reduce` that builds an
  object is `Object.groupBy`, `Map.groupBy` (ES2024), or `Object.fromEntries`.
- `some` over `filter(...).length > 0`; `find` over `filter(...)[0]`; `at(-1)` over
  `[arr.length - 1]`.
- `toSorted`, `toReversed`, `with` (ES2023) over the mutating forms.
- `Map` and `Set` for keyed lookups and membership; plain objects for records with known
  keys. `Record<string, T>` keyed by ids is a `Map`. `Set.union`, `intersection`,
  `difference` (ES2025) replace hand-rolled loops.
- `structuredClone` for deep copies, never `JSON.parse(JSON.stringify(x))`.
- Push filtering, sorting, counting and existence into SQL when the data comes from SQL.

## Boundaries and validation

- Validate at every trust boundary, once: HTTP body, query, params and headers you read;
  env; queue messages; third-party API responses; rows from raw SQL. Types are erased;
  everything crossing a process boundary is `unknown` at runtime.
- Parse, do not validate: the parser's return value is the typed value. `const body =
  BodySchema.parse(req.body)`, never check-then-cast.
- `safeParse` at the HTTP edge (map failure to 400 with the issues); `parse` at startup
  (die with a readable message).
- Zod 4 specifics an assistant gets wrong: `z.email()`, `z.uuid()`, `z.iso.datetime()`
  are top-level (the `.email()` method is deprecated); one `error` option replaces
  `message`, `invalid_type_error`, `required_error` and `errorMap`; `z.strictObject()`
  and `z.looseObject()` replace `.strict()` and `.passthrough()`; `.default()` applies
  to the output type without parsing; `z.prettifyError()` for logs; `z.toJSONSchema()`
  for OpenAPI. `zod/mini` is for bundle-size-sensitive edge code, not Node services.
  Valibot and ArkType are fine alternatives; one validator per repo.
- A schema failure on a third-party response is a 502 and a bug report, not a 400.
- Express 5: `req.body` is `undefined` until a body parser runs; `req.query` is a
  read-only getter with the `simple` parser (no nested objects); validate `req.query`
  too, since attackers send arrays where strings are expected. Fastify: use
  `@fastify/type-provider-zod` so route `schema` types `request.body`.
- Env: parsed in one `config.ts` at startup, exported frozen. `node --env-file=.env`
  locally instead of `dotenv`; `process.loadEnvFile()` is stable since Node 24.

## Errors

- Throw only `Error` subclasses; never strings, plain objects or `Promise.reject('x')`.
  Stack traces, `cause` chains and framework error handlers depend on it.
- Custom errors set `name`, carry a `code` discriminant and accept `{ cause }`:
  ```ts
  export class NotFoundError extends Error {
    readonly code = 'NOT_FOUND' as const;
    constructor(message: string, options?: ErrorOptions) {
      super(message, options);
      this.name = 'NotFoundError';
    }
  }
  ```
  Wrap with `new RepositoryError('insert failed', { cause: err })` so the original stack
  survives; pino and the log platform serialise `cause` chains.
- `catch (e)` is `unknown` under `strict`. Narrow with `instanceof Error` or
  `Error.isError(e)` (ES2026, Node 24+, realm-safe). Wrap non-Errors:
  `new Error(String(e), { cause: e })`. Never `catch (e: any)`, never `e.message`
  without narrowing.
- Operational errors (validation, not found, timeout, upstream 5xx) are expected and get
  mapped to a response. Programmer errors (bugs) are logged and the process exits so the
  orchestrator restarts it; state after an uncaught exception is unreliable.
- Result unions for expected, domain-level outcomes a caller must branch on
  ("insufficient funds"); exceptions for unexpected or unrecoverable failures. Do not
  wrap every function in a Result unless the team has adopted a library for it
  (`neverthrow`, Effect); that is an explicit team decision.
- Catch where you can act: retry, translate, add context. Catch-log-rethrow at every
  layer produces four log lines for one failure. Let it travel to the error handler,
  which logs once with the request id.
- One error handler maps domain errors to HTTP as RFC 9457 problem details
  (`application/problem+json`; `type`, `title`, `status`, `detail`, `instance`, plus an
  `errors` array for validation). Generic body for any status 500 and above; the real
  error goes to the log. Never leak stack traces, SQL text or internal hostnames.
- Express 5: async handler rejections reach the error middleware natively, so delete
  `express-async-errors` and `.catch(next)`. Still uncaught: errors in detached work
  (`setImmediate`, stream `error` events). The error middleware keeps the four-argument
  signature and is registered last. Other 5 traps: `app.del` removed, `res.send(status)`
  removed (use `res.sendStatus`), wildcards are named (`/*splat`), optional params use
  braces (`/:file{.:ext}`), `req.params` has a null prototype.
- Fastify: the default error handler sends `error.message` for every status including
  500. Always set a root `setErrorHandler` that returns a generic body for 500 and above.
- Every `EventEmitter` and stream needs an `error` listener or the process crashes.

## Async

- `async`/`await` everywhere. `.then` chains and callbacks are for library edges only;
  `util.promisify` or `events.once` turns them into promises.
- No floating promises. Every promise is awaited, returned, or explicitly discarded as
  `void doThing().catch(log)` with a reason; never a bare `void doThing()`.
  `no-floating-promises` and `no-misused-promises` (catches `if (promise)` and async
  callbacks passed where `void` is expected) enforce it.
- `return await` inside `try`/`catch`, or the rejection escapes the catch. Configure
  `return-await` to `in-try-catch` or `always`.
- Unhandled rejections crash the process since Node 15. Register one
  `process.on('unhandledRejection')` and one `uncaughtException` handler that log at
  fatal and exit; never swallow and continue.
- Independent work runs together with `Promise.all`. Dependent work, or work against a
  small pool, stays sequential: converting a loop of writes to `Promise.all` changes
  ordering and can exhaust a pool of five. Bound any `Promise.all` over user-sized input
  with `p-limit` or `p-map`.
- `Promise.allSettled` when partial failure is expected and the rest must proceed;
  `Promise.any` to race redundant sources. `Promise.race` for timeouts is wrong because
  the loser keeps running; `AbortSignal.timeout(ms)` cancels it.
- `AbortSignal` is the cancellation primitive. Pass it through service and repository
  so a cancelled request stops downstream work; combine with `AbortSignal.any([...])`.
  Let `AbortError` propagate as itself rather than converting it to a 500.
- Streams: `for await (const chunk of readable)` and `stream/promises.pipeline` instead
  of manual `.on('data')`; `pipeline` propagates errors and destroys every stream.
- `setImmediate` to yield to I/O when breaking up work. `process.nextTick` and
  microtasks run before I/O and starve the loop when recursive; never use `nextTick`
  for "later".
- `await using` (TS 5.2, Node 24) for resources that must be released: connections,
  file handles, locks.

## Performance on the event loop

Node runs one request's CPU work at a time per process. Anything over a few
milliseconds of CPU per request is paid by every concurrent request.

- `JSON.parse` and `JSON.stringify` are synchronous and linear. Cap body sizes
  (`express.json({ limit: '100kb' })`, Fastify `bodyLimit`), stream large uploads,
  paginate responses, use a streaming parser for bulk imports.
- No `*Sync` fs or crypto calls after startup (`readFileSync`, `pbkdf2Sync`,
  `execSync`). Startup-time config reads are fine.
- Regex on untrusted input is CPU on the loop and a ReDoS vector. Cap input length,
  avoid nested quantifiers, use `RegExp.escape` (ES2025, Node 24+) when building
  patterns from strings.
- Unbounded in-memory caches leak. Use an LRU with `max` and `ttl`, or Redis. Watch
  `Map`s keyed by user or request id.
- One `undici.Agent` or `Pool` per upstream with explicit `connections`,
  `keepAliveTimeout`, `headersTimeout` and `bodyTimeout`. A client per request leaks
  sockets and defeats TLS session reuse. Always set timeouts; defaults are generous
  or infinite.
- Size database pools explicitly, set `connectionTimeoutMillis` and
  `statement_timeout`. N+1 queries are the usual cause of a slow endpoint; batch with
  `IN (...)` or a dataloader.
- Logging in hot paths is cheap only when lazy: pass objects to pino (it serialises
  only when the level is on); never pre-build strings for a debug line.
- CPU-bound work (hashing, image or JSON crunching) goes to `worker_threads` or to
  horizontal replicas; `child_process` is for other binaries or crash isolation. Never
  `exec` with user input; `execFile` with an args array.
- Measure before optimising: `--cpu-prof`, `clinic flame`,
  `perf_hooks.monitorEventLoopDelay`. Native `Array`, `Set`, `Object.groupBy` and
  `structuredClone` replace most of lodash and are faster in current V8.
