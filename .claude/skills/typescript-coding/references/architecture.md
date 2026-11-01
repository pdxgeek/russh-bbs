# Architecture: layout, wiring, config, tooling, logging, operations

How a Node service is put together here. Sources: Node Best Practices (goldbergyoni)
sections 1, 5, 6 and 7, the Node.js API docs (`typescript`, `modules`, `packages`,
`http`, `process`), Node's recommended tsconfig, Matt Pocock's TSConfig cheat sheet,
TkDodo "Please stop using barrel files", the Google TypeScript Style Guide, awilix docs,
typescript-eslint docs, the OWASP Node.js cheat sheet, Datadog log attribute conventions.

Contents: Project layout · Layers · Dependency wiring · Config and env · tsconfig ·
Lint and format · Logging · Security basics · Process lifecycle

## Project layout

The repo's existing layout wins. A newcomer should be able to predict where a thing
lives before opening a file, and that only works when every file follows the same rule.

**For a new service, package by feature**, with the three layers inside each feature:

```
src/
  app.ts                       # composition root: builds the graph, mounts routers; no logic
  server.ts                    # listens, health endpoints, graceful shutdown; reads the port
  config.ts                    # parses process.env with a schema at startup; exported frozen
  orders/
    orders.routes.ts           # paths to handler functions
    orders.controller.ts       # HTTP in, HTTP out: parse, call the service, return
    orders.service.ts          # rules, orchestration, the transaction boundary
    orders.repository.ts       # SQL only; returns rows mapped once
    orders.schema.ts           # zod schemas and the types inferred from them
    orders.types.ts            # domain types and ports (the interfaces the service needs)
    orders.service.test.ts     # tests sit beside the code they test
  shared/
    errors.ts                  # the error classes and the HTTP mapping
    logging/logger.ts          # the one pino instance and its configuration
    db/                        # the query builder instance, generated types, transaction helper
    http/                      # error handler middleware, request context, problem details
test/
  integration/                 # container-backed tests, a separate Vitest project
  fixtures/                    # row builders; one per table
```

A small app can keep top-level `controllers/`, `services/`, `repositories/` buckets with
one file per aggregate inside each; the layer rules below are identical. Move to feature
folders when a change routinely touches three buckets, and move whole features, never
half.

- One module exports one concern and is named for what it exports:
  `orders.repository.ts` exports order repository functions and nothing else.
- Named exports only; no default exports. They are grep-able, refactor-safe and never
  drift between `import foo` and `import { foo }`.
- No internal `index.ts` barrel files. They load every sibling eagerly (one measured
  codebase cut startup 68% by removing them), create import cycles when a sibling
  imports from its own folder's barrel, and defeat tree shaking. The one acceptable
  barrel is a published package's entry point. Import the concrete file.
- Relative imports carry the extension: `./orders.service.ts` with
  `rewriteRelativeImportExtensions` (TS 5.7+), or `./orders.service.js` in a repo that
  emits that way. Pick one and lint it. Never bare `./orders.service`; Node will not
  guess.
- Internal aliases use package.json `imports` (`"#app/*": "./src/*.ts"`), which Node and
  TypeScript both honour. Not `paths` and `baseUrl`; Node ignores `paths` and `baseUrl`
  is an error in TypeScript 7.
- Circular imports surface as `undefined` at runtime or "Cannot access before
  initialization". Lint with `import-x/no-cycle`; break them by moving shared types to a
  `*.types.ts` file or by passing the dependency instead of importing it.
- `shared/` or `utils/` with more than a handful of files is a smell: each cluster is a
  domain module waiting for a name.
- Generated files carry `.generated.ts`, live next to what generates them, and are never
  edited by hand.
- Depth of three below `src/` is the ceiling.

## Layers

Dependency direction is one way: route, controller, service, repository, db. Each layer
knows only the one below it.

| Layer | Knows about | Never contains |
|---|---|---|
| Route | the framework, controller functions | logic of any kind |
| Controller | `req`/`res`, schemas, services | SQL, business rules, status codes chosen by hand |
| Service | domain types, repository ports, the transaction helper, the clock, the logger | `req`/`res`, HTTP status codes, SQL |
| Repository | the query builder, generated table types, row mappers | business rules, HTTP, logging above debug |
| Domain / types | nothing with I/O | imports from any other layer |

- Validation happens once, at the boundary. The service receives a typed value and
  trusts it. Re-validating inside is noise; checking a rule the schema cannot express
  (this property belongs to that company) is the service's job.
- The transaction boundary is the service. A repository function takes the executor
  (`db` or `trx`) as a parameter so it works inside or outside a transaction.
- Row mappers (`toOrder(row): Order`) live with the repository. The service never sees
  snake_case or `null`.
- Errors cross layers as typed classes and are mapped to HTTP in one place, the error
  handler. Controllers never write `res.status(404)`.
- Request-scoped values (request id, actor) travel as a `ctx` parameter.
  `AsyncLocalStorage` is acceptable for cross-cutting concerns only (log correlation,
  tracing), never as a hidden parameter for business data.

## Dependency wiring

- One composition root, `app.ts`, builds the whole graph in order: config, then
  infrastructure (pool, HTTP clients, logger), then repositories, services, routers.
  Nothing else constructs a collaborator or imports a concrete repository. Swapping the
  real database for a container, or the clock for a fake, is then a one-line change.
- Services are factory functions:
  ```ts
  export function createOrderService(deps: {
    orders: OrderRepository;
    clock: () => Date;
    logger: Logger;
  }): OrderService { ... }
  ```
  The `deps` type is the narrowest interface the service uses, declared next to the
  service (the consumer owns the port). The infrastructure module implements it. A test
  passes a two-line fake; nothing is module-mocked.
- No module-level singletons or mutable state: no `export const db = createPool()`, no
  `export let cache`. Import order becomes initialisation order, tests share state, and
  hot reload breaks. Pure constants and the parsed config object are the exceptions.
- A DI container earns its place only with a large graph (dozens of registrations),
  per-request scopes with disposal, or a plugin architecture. Then awilix: no decorators,
  works with factory functions. `tsyringe` and `inversify` need decorators and
  `reflect-metadata`, which Node's type stripper cannot run. Never resolve from the
  container inside business code; that is a service locator.

## Config and env

- `process.env` is read in exactly one module, parsed with a schema at startup, and
  exported as a frozen typed object. Missing keys fail the boot with all of them listed.
- `node --env-file=.env` (or `--env-file-if-exists`) locally; no `dotenv`.
  `process.loadEnvFile()` is stable since Node 24. `.env` is never committed.
- `NODE_ENV=production` in production; the frameworks behave differently without it.
- Secrets come from env or a secrets manager, never from code, and are redacted at the
  logger.

## tsconfig

Node's own recommended block, extended:

```json
{
  "compilerOptions": {
    "target": "esnext",
    "module": "nodenext",
    "lib": ["es2024"],
    "types": ["node"],
    "rootDir": "./src",
    "outDir": "./dist",
    "strict": true,
    "noUncheckedIndexedAccess": true,
    "exactOptionalPropertyTypes": true,
    "noImplicitOverride": true,
    "noImplicitReturns": true,
    "noFallthroughCasesInSwitch": true,
    "verbatimModuleSyntax": true,
    "erasableSyntaxOnly": true,
    "rewriteRelativeImportExtensions": true,
    "skipLibCheck": true
  }
}
```

- `module: nodenext` is the only setting that models Node's real ESM/CJS resolution,
  `exports` and `require(esm)`. `node20` (stable in TS 5.9) freezes the target. Never
  `commonjs` with `moduleResolution: node` for new code.
- `verbatimModuleSyntax` forces `import type`, which Node's stripper needs, and makes
  `isolatedModules` redundant.
- `erasableSyntaxOnly` turns `enum`, value `namespace`s, parameter properties and
  `import x = y` into compile errors; Node cannot run any of them.
- `lib` without `dom` for a server, so `fetch` and `setTimeout` get Node signatures and
  `document` does not exist. `types: ["node"]` because TypeScript 6 defaults `types` to
  `[]`.
- `rootDir` set explicitly, or TypeScript 6 emits into `dist/src/`.
- `exactOptionalPropertyTypes` is the contested one: it distinguishes `{ a?: string }`
  from `{ a: string | undefined }` and breaks `{ ...o, a: maybeUndefined }`. Keep it on
  for new code and write `a?: string | undefined` where spreading needs it.
- Do not set `esModuleInterop` (always on from TS 6), `baseUrl`, `paths`,
  `downlevelIteration`, `importsNotUsedAsValues`.
- `"type": "module"` in package.json; `npm init` on npm 11 writes `commonjs`, fix it.
  `.mts`/`.cts` force a format; `.ts` follows the nearest package.json.
- Decorators do not strip. Avoid them unless a framework owns the build (NestJS).
- Published packages declare `exports` with `types` conditions first, so deep imports
  fail and internals stay internal.

## Lint and format

- Formatting is never discussed in review. Prettier, or Biome as a faster
  Prettier-compatible formatter, with `--check` in CI. Where the repo has no config:
  2 spaces, single quotes, semicolons, trailing commas, 100 columns.
- ESLint 9 flat config with `tseslint.configs.strictTypeChecked` and
  `stylisticTypeChecked`, `parserOptions.projectService: true`. The strict set catches
  exactly the mistakes an assistant makes. Lint errors block merge.
- Rules worth naming: `no-floating-promises`, `no-misused-promises`, `await-thenable`,
  `return-await` (`in-try-catch`), `switch-exhaustiveness-check` with
  `considerDefaultExhaustiveForUnions: false`, `no-unnecessary-condition`,
  `prefer-nullish-coalescing`, `prefer-optional-chain`, `consistent-type-imports` and
  `consistent-type-exports` (`fixStyle: 'inline-type-imports'`),
  `no-unnecessary-type-assertion`, the `no-unsafe-*` family, `restrict-template-
  expressions`, `no-confusing-void-expression`, `no-deprecated`, `no-console`.
  Plus `eslint-plugin-import-x` (`no-cycle`, `no-default-export`) and
  `eslint-plugin-unicorn` (`prefer-node-protocol`, `filename-case`, `no-array-for-each`).
- Version caveat: typed linting needs the TypeScript 6 API. While `tsc` runs on 7.x,
  install `typescript@npm:@typescript/typescript6` for ESLint, or expect peer errors.
  Biome's type-aware rules run on its own inference and cover roughly three quarters of
  typescript-eslint's; fine for libraries, keep typescript-eslint for services.
- `// eslint-disable` and `@ts-expect-error` always carry a reason.
- Imports ordered: `node:` built-ins, external packages, internal modules; type-only
  imports last in each group; one blank line between groups. No unused imports.

## Logging

Logs are structured JSON on stdout, one object per line, shipped to Datadog. A log line
is a record with a message, not a sentence with values glued in.

- One logger, pino, created in `shared/logging/logger.ts` and injected. `console.*`
  never appears in `src/`.
- Every line has a `message` field. Datadog's explorer, patterns and monitors key on
  `message`; pino's default key is `msg`, so set `messageKey: 'message'`.
- The message is a short, constant sentence naming the event: `'screening extraction
  completed'`. Values go in fields: `logger.info({ propertyId, pageCount, durationMs },
  'screening extraction completed')`. Interpolating ids into the message creates a
  unique message per request and defeats pattern grouping.
- Fields Datadog understands without remapping: `level` as a string
  (`formatters.level: (label) => ({ level: label })`), `timestamp` in ISO 8601,
  `service`, `env` and `version` as base fields, errors under `error` with `kind`,
  `message` and `stack` (a serializer maps `err` to that shape). With dd-trace log
  injection on, `dd.trace_id` and `dd.span_id` land on every line.
  ```ts
  export const logger = pino({
    messageKey: 'message',
    timestamp: () => `,"timestamp":"${new Date().toISOString()}"`,
    formatters: { level: (label) => ({ level: label }) },
    base: { service: config.serviceName, env: config.env, version: config.version },
    redact: ['req.headers.authorization', '*.password', '*.token'],
    serializers: { err: toDatadogError },
  });
  ```
- `requestId` on every line within a request, through a child logger created in the
  request-context middleware and passed down, never through a global.
- Log once per event, at the layer that owns it. A failure is logged by the error
  handler, once, with the request id. Repositories log at `debug` at most. Services log
  business events at `info`. `warn` means someone should look.
- Never log secrets, tokens, full request bodies or document text. Redact at the logger
  with pino's `redact`, not by remembering at each call site.
- Local development uses `pino-pretty` through a transport switched on `NODE_ENV`; the
  shipped format is always raw JSON.

## Security basics

- Validate and bound every input: schema-validate body, query, params and the headers
  you read; cap body size (`express.json({ limit: '100kb' })`, Fastify `bodyLimit`).
- Prototype pollution: never deep-merge user input into objects, never bracket-access a
  plain object with a user-controlled key; use `Map` or `Object.create(null)`, freeze
  config. Express 5 gives `req.params` a null prototype and a `simple` query parser for
  this reason.
- SQL is parameterised, always, through the query builder or `$1` placeholders.
  Identifiers from user input (sort columns) go through an allowlist.
- No `eval`, `new Function`, or `node:vm` as a sandbox. No `child_process.exec` with
  interpolation; `execFile` with an args array. File paths built from user input go
  through `path.resolve` plus a prefix check.
- `helmet` (or `@fastify/helmet`) for headers, including a `default-src 'none'` CSP on
  an API. Rate-limit auth and expensive routes with a shared store when multi-instance.
  CORS allowlist, never `*` with credentials. Cookies `httpOnly`, `secure`, `sameSite`.
- Dependencies: commit the lockfile, `npm ci` in CI, `npm audit` or an equivalent with
  a documented triage, Renovate or Dependabot rather than `latest`. Run as non-root.
- Error responses: generic body for 500 and above; the real error, with request id, goes
  to the log.

## Process lifecycle

- Graceful shutdown on `SIGTERM` and `SIGINT`, handled once, in `server.ts`:
  1. flip readiness to unhealthy;
  2. `server.close()` to stop accepting (Node 19+ also drops idle keep-alive sockets);
  3. await in-flight requests;
  4. close pools, queues and clients;
  5. `process.exit(0)`.
  Arm a force-exit timer shorter than the orchestrator's grace period that calls
  `server.closeAllConnections()` and exits 1. In Docker use the exec-form `CMD ["node",
  ...]` or signals never reach Node.
- Liveness (`/healthz`: the process is up, no dependencies) and readiness (`/readyz`:
  database ping, queue connection) are separate endpoints. A liveness check that
  depends on downstream services causes restart storms.
- `unhandledRejection` and `uncaughtException` handlers log at fatal and exit. The
  orchestrator restarts the process; continuing with unknown state is worse.
- Horizontal replicas over in-process `cluster` in containers. `worker_threads` only
  when the hot path is CPU-bound.
- Dev loop: `node --watch`, `node --env-file`, `node --test` or Vitest. No `nodemon`,
  no `ts-node`; `tsx` only when a feature the stripper lacks is needed.
