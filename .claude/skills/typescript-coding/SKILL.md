---
name: typescript-coding
description: How to write TypeScript on Node in this codebase - the house standard for any TypeScript, JavaScript or Node work, from a one-line fix to a new service. Use it every time you write, change, review or test TypeScript - new modules, Express or Fastify handlers, GraphQL resolvers, repository functions, scripts, config, tests - even when the user only says "add a function", "fix this bug", "write the handler for X" or "clean this up" and never mentions TypeScript, style or quality. Also use it when reviewing a diff that shows any, as casts, the ! operator, enum, forEach pushing into arrays, await inside loops, SQL or business rules in a controller, boolean flag parameters, console.log, or vi.mock of the project's own modules. The refactoring sub-skill inside covers converting existing code safely.
---

# Writing TypeScript on Node

This is how TypeScript gets written here: strict types that prove the code, small named
functions, one home for every rule, boundaries that parse, errors that travel to one
handler, and tests that exercise behaviour. The standard is the same whether the task is
a new service or a two-line fix; a small change still lands in a file that someone has
to read tomorrow.

The body of this file is the standard in brief. The references carry the depth and the
reasons; open them when the task touches their area.

| When the task involves | Read |
|---|---|
| Any non-trivial type, function, async flow or error path | `references/style.md` |
| New modules, project layout, dependency wiring, config, logging, tsconfig, lint, security, shutdown | `references/architecture.md` |
| Writing or changing tests, deciding what to fake, database or HTTP tests | `references/testing.md` |
| Changing code that already works: cleanups, modernising, "make this a map" | `references/refactoring.md` (the refactoring sub-skill) |

## How to work

**Before writing.** Read the surrounding module and one neighbour to learn the repo's
conventions: file naming, import style, how dependencies arrive, how errors are typed,
which validator and logger are in use. The repo's convention wins over this document
wherever they differ, because consistency inside a codebase is worth more than any single
rule. Check the installed majors of TypeScript, Node, the validator and the test runner
before using an API that changed between versions (see the version notes in each
reference).

**While writing.** Start from the types: the input shape, the output shape, the error
cases. Then write the function so the compiler proves it. Keep each function at one level
of abstraction, name things for what they mean, and put each piece of knowledge in one
place. Parse at the boundary, trust inside. Let errors propagate to the one layer that
can act on them.

**Before calling it done.** Run the project's type check, lint and the tests that cover
the touched code. Node runs TypeScript without checking it, so "it runs" is never
evidence that it compiles. State what was verified and how; when something could not be
verified, say so first.

## The standard in brief

### Types prove the code

- Strict mode on. No `any`; `unknown` for values not yet examined, narrowed by a guard,
  `instanceof`, `in`, or a schema parse. `any` spreads through inference and switches
  the checker off for everything it touches.
- No non-null `!` and no `as` casts except `as const` and a commented cast at a verified
  boundary. Both are unverified claims; a guard or an honest `T | undefined` is the fix.
  `@ts-expect-error` with a reason for library bugs, never `@ts-ignore`.
- Infer locals; annotate every exported function's parameters and return type. The
  return annotation is documentation and catches drift when a branch is added.
- Model states as discriminated unions, not bags of optional fields. Switch on the
  discriminant and finish with `default: x satisfies never` so adding a variant lists
  every place that must learn about it.
- No `enum`, no `namespace` with values, no class parameter properties. Node's type
  stripper cannot run them and `erasableSyntaxOnly` rejects them. Use an `as const`
  object plus a derived union, or a plain string-literal union.
- `satisfies` to check a literal without widening it. `readonly` on arrays and objects
  the function does not mutate. Generics only when a type parameter relates two
  positions; one used once is `unknown` in disguise.
- `interface` for object shapes, `type` for unions and aliases. `import type` for
  anything used only as a type, or Node throws at runtime.
- Branded types for ids (`UserId`, `OrderId`) minted only at the boundary, so an
  `OrderId` cannot be passed where a `UserId` is expected.

### Nullability

- `undefined` is the internal "absent". Database and JSON `null` are mapped to
  `undefined` once, in the row mapper or schema. Internal code never checks both.
- `??` for defaults, never `||`, unless `0`, `''` and `false` are meant to fall through.
  `?.` chains replace guard ladders when every missing link means the same thing; a
  guard that does something distinct, like logging, stays.
- `noUncheckedIndexedAccess` is on, so `arr[0]` and `record[key]` are `T | undefined`.
  Handle it with a guard, a default or `.at()`, never `!`.

### Functions and names

- One level of abstraction per function: it orchestrates named steps or it is a step.
  A body that needs section comments is several functions. Around twenty lines is the
  signal to look for the seam.
- Guards first, early return, happy path at the shallowest indent. At most three
  positional parameters, then an options object. A boolean parameter that switches
  behaviour is two functions.
- No argument mutation; `toSorted`, `toReversed`, `with`, spread. Pure functions in the
  middle, side effects at the edges.
- One shape of return. A result union (`{ ok: true, value } | { ok: false, error }`)
  when the caller must branch on an expected outcome; a thrown typed error when it
  cannot recover. A function does one or the other, never both.
- Functions are verbs that state the effect (`loadActivePrompt`, `toPolicyRow`);
  booleans read as predicates (`isMultiUnit`, `hasPolicy`); collections are plural; a
  `Map` is named by what it indexes (`policiesByPropertyId`). No abbreviations beyond
  `id`, `url`, `db`, `req`, `res`, `ctx`. Comments say why, never what.

### Modules and layers

- Follow the repo's layout. For a new one: package by feature, with routes, service,
  repository, schema and tests side by side inside the feature folder. Buckets named
  `controllers/`, `services/` at the top level suit only a small app.
- Dependency direction is one way: route, controller, service, repository, db. Routes
  and controllers parse, validate and map errors to HTTP. Services hold rules and own
  the transaction. Repositories hold SQL and return rows mapped once at the edge. No
  `req`/`res` below the controller, no SQL above the repository.
- Dependencies arrive as parameters. Services are factory functions taking a `deps`
  object typed as the narrowest interface they use; one composition root (`app.ts`)
  builds the graph. No module-level singletons (`export const db = createPool()`), no
  decorator-based DI. This is what makes tests possible without mocking modules.
- Named exports only; no default exports; no internal `index.ts` barrel files. Import
  the concrete file with its extension (`./orders.service.ts`, or `.js` in a repo that
  emits that way), never bare `./orders.service`.
- `process.env` is read in one config module, parsed with a schema at startup, and
  exported as a frozen typed object. A missing variable fails the boot, not a request.
- Built-ins with the `node:` prefix. Built-in `fetch`, `AbortSignal`, `structuredClone`,
  `Array.fromAsync`, `Object.groupBy`; no `axios`, `node-fetch` or `lodash` for what
  the platform does.

### Boundaries, errors and async

- Parse, do not validate: at every trust boundary (HTTP body, query and params, env,
  queue messages, third-party responses, raw SQL rows) a schema parse returns the typed
  value and `z.infer` is the only source of the type. `safeParse` at the HTTP edge,
  `parse` at startup. Never write the interface and the schema separately.
- Errors are classes extending `Error` with `name` set and `cause` when wrapping.
  Throw only `Error` subclasses. `catch (e)` gives `unknown`; narrow it. Expected
  failures are mapped to a response; unknown ones are logged once and, when the
  process state is unreliable, the process exits and the orchestrator restarts it.
- Domain errors cross layers as typed classes and are mapped to HTTP in exactly one
  place, the error handler, as RFC 9457 problem details. Controllers never write
  `res.status(404)` by hand. Never leak stack traces or SQL to the client.
- `async`/`await` everywhere. No floating promises: await, return, or
  `void p.catch(log)` with a reason. `return await` inside `try`. `Promise.all` for
  independent work, `Promise.allSettled` when partial failure is expected, bounded
  concurrency (`p-limit`) over user-sized input, sequential when items depend on each
  other or the pool is small. `AbortSignal.timeout` for timeouts, never `Promise.race`.
- Nothing synchronous and slow after startup: no `*Sync` fs or crypto calls, no
  unbounded `JSON.parse` of request bodies, no regex on unbounded untrusted input.
  The event loop is shared by every request.

### Logging

One structured pino logger, created once and injected. Every line has a constant
`message` naming the event and the values as fields, so the log platform can group
patterns and facet: `logger.info({ propertyId, durationMs }, 'screening extraction
completed')`. `requestId` through a child logger. Log once per event at the layer that
owns it; failures are logged by the error handler, once. Secrets redacted at the logger.
`console.*` never appears in `src/`.

### Tests

Vitest. `describe` per unit, `it` named as the behaviour it proves in plain present
tense. Arrange, act, assert. Fakes over mocks: pass a two-line fake through the `deps`
parameter; never `vi.mock` a module under `src/`. Mock only the network edge, with MSW
set to fail on unhandled requests. Repositories are tested against a real database in a
container. A test that cannot fail is deleted. Details in `references/testing.md`.

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
