# Refactoring existing code (sub-skill)

Changing code that already works is a different job from writing new code. The standard
in SKILL.md still applies to the result; this file is about getting there without
changing what the code does. Read it for cleanups, modernisations, "make this a map",
and reviews of someone else's diff.

Contents: Method · The moves, by smell · Worked examples · Reporting

## Method

1. **Equivalence first.** Before changing anything, write down what the old code does
   that the new code must still do: evaluation order, which awaits ran in sequence, side
   effects, distinct log lines, which errors escape and with what status. A rewrite that
   turns sequential writes into `Promise.all`, or merges two different "not found"
   branches, is a bug wearing a cleanup's clothes. When behaviour does change, verify it
   is harmless and say so in the report, or keep the original shape.
2. **Delete before you convert.** Dead branches, checks the type already proves, and
   arms subsumed by another arm come out first. The best refactor of redundant code is
   removal, and the conversion that follows is simpler for it.
3. **One move per smell.** The table below pairs each shape with the move that replaces
   it. Apply the move, run the tests, then look for the next smell. Mixing several moves
   in one step hides which one broke the test.
4. **Keep the imperative form when it is honest.** A `for...of` whose body mutates
   locals, breaks early, or awaits sequentially on purpose stays a loop. Never force a
   chain that needs contortions; fix the reason for the contortion (usually a missing
   named helper or a dependency that should be injected).
5. **Named helpers make pipelines readable.** A chain reads as a sentence only when each
   stage is a short expression or a well-named call. Extract fat callbacks into named
   functions. This is also the standard unblocking move when a callback cannot do what
   the body did (throw a typed error, log once).
6. **Verify with the project's tools.** Type check, lint and the tests scoped to the
   touched code run green before the change is called safe. Add a test first when the
   behaviour being preserved has none.

## The moves, by smell

| Smell in the code | Move |
|---|---|
| `any`, `Record<string, any>`, `as SomeType` on a row or JSON | `unknown` plus a schema or type guard at the boundary, typed from there (example 1) |
| `value!` or `as Foo` to silence a maybe-undefined | Narrow with a guard, or throw a typed `NotFoundError` (example 2) |
| Ladder of `if (!a) return; if (!a.b) return;` | `a?.b ?? fallback`; keep a guard only where the branch does something distinct (example 3) |
| `forEach` pushing into an array; `reduce` building an object | `map`/`filter`/`flatMap`; `Object.groupBy`/`Map.groupBy`; `Object.fromEntries` (example 4) |
| `await` inside a `for` over independent items | `Promise.all`, or bounded concurrency; keep the loop when items depend on each other or the pool is small (example 5) |
| SQL, validation and rules in one handler | Controller, service, repository, with the transaction in the service (example 6) |
| `doThing(x, true)`; string unions typed as `string` | Two functions or an options object; `as const` object plus derived union, exhaustive `switch` (example 7) |
| `vi.mock('../src/...')` of the project's own module | Inject the dependency through `deps`; test the real function with a fake (example 8) |
| Same validate-and-400 block in every handler | One schema, one `parseBody` helper that throws a typed error the error handler maps (example 9) |
| A function with phases separated by blank lines or comments | Named steps at one level of abstraction (example 10) |
| `.then().catch(err => logger.error(err))`; catch-log-rethrow at every layer | `async`/`await`; let it propagate to the error handler; wrap with `cause` (example 11) |
| `console.log`, values interpolated into a log message, a line with no message | Injected pino logger, constant message naming the event, values as fields (example 12) |
| `(await repo.findAll(...)).length > 0` | An `exists` query (example 13) |
| `enum`; class parameter properties; `namespace` with values | `as const` object plus `typeof` union; declared and assigned fields; a module |
| `null` flowing through internal code | `undefined` inside; map database `null` once at the repository edge |
| `let` assigned once | `const` |
| `require`, missing `node:` prefix, `process.env` outside config | ESM `import`, `node:fs/promises`, the one config module |
| Bare relative import `./foo` | `./foo.ts` (with `rewriteRelativeImportExtensions`) or `./foo.js`, matching the repo |
| `express-async-errors`, `.catch(next)` in every handler | Delete on Express 5; async rejections reach the error middleware natively |
| `Promise.race` against a timer | `AbortSignal.timeout`, passed through to the work |
| `filter(Boolean) as T[]` | `filter((x) => x !== undefined)`; TS 5.5 infers the predicate |
| `z.string().email()`, `.strict()`, `message:` in a schema | `z.email()`, `z.strictObject()`, `error:` (Zod 4) |
| Import of an `index.ts` barrel inside `src/` | Import the concrete file |

## Worked examples

Real rewrites, genericised. Each shows the shape to hunt for and the shape to produce,
with the reasoning that decided it. Every one preserved behaviour, and that was verified
with the project's tests before it was committed, not assumed.

### 1. A typed boundary instead of `any` rows

Hunt for: `any` or `as SomeRow` on the result of a query or `JSON.parse`.

```ts
// Before
const rows: any = await db.query('SELECT ...');
return rows.map((r: any) => ({ id: r.id, name: r.property_name, lat: r.lat }));

// After
const rows = await db
  .selectFrom('properties')
  .select(['id', 'name', sql<number | null>`ST_Y(geocode::geometry)`.as('latitude')])
  .where('company_id', '=', companyId)
  .execute();

return rows.map(toPropertySummary);

function toPropertySummary(row: PropertySummaryRow): PropertySummary {
  return { id: row.id, name: row.name, latitude: row.latitude ?? undefined };
}
```

The generated table types make the row type real, the mapper is the one place
snake_case and `null` exist, and a renamed column is now a compile error instead of an
`undefined` in production.

### 2. Narrow or throw instead of `!`

Hunt for: `value!`, `as Foo` to quiet a maybe-undefined, `Map.get(k)!`.

```ts
// Before
const property = await loadProperty(id);
return property!.companyId;

// After
const property = await loadProperty(id);
if (!property) {
  throw new NotFoundError('property', id);
}
return property.companyId;
```

The `!` was a claim with no evidence. The guard makes the failure explicit, gives it a
type the error handler maps to 404, and the happy path reads exactly as before.

### 3. Collapse a guard ladder, keep the guard that differs

Hunt for: a stack of `if (!x) return` where most arms mean the same thing.

```ts
// Before
if (!property) return undefined;
if (!property.payoutSetting) {
  logger.warn({ propertyId: property.id }, 'payout setting missing');
  return undefined;
}
if (!property.payoutSetting.account) return undefined;
return property.payoutSetting.account.email;

// After
if (!property) return undefined;
if (!property.payoutSetting) {
  logger.warn({ propertyId: property.id }, 'payout setting missing');
  return undefined;
}
return property.payoutSetting.account?.email;
```

Two of the three empties were identical and collapsed into `?.`. The one that logged is
behaviour, so its guard stayed. Merging distinct empties silently is a bug.

### 4. Collectors, not accumulators

Hunt for: `forEach` pushing into an array, `reduce` building an object keyed by id.

```ts
// Before
const byProperty: Record<string, Policy[]> = {};
policies.forEach((p) => {
  if (!byProperty[p.propertyId]) byProperty[p.propertyId] = [];
  byProperty[p.propertyId].push(p);
});

// After
const policiesByPropertyId = Map.groupBy(policies, (policy) => policy.propertyId);
```

`Map.groupBy` states the intent, keeps the key type as `number` instead of coercing to
string, and cannot produce the `undefined` the `Record` version allowed.

### 5. Parallel when independent, sequential when not

Hunt for: `await` inside `for` over items that do not depend on each other; also the
reverse, `Promise.all` over writes that must happen in order.

```ts
// Before: three independent reads, run one after another
const images = await loadImages(propertyId);
const floorPlans = await loadFloorPlans(propertyId);
const policies = await loadPolicies(propertyId);

// After
const [images, floorPlans, policies] = await Promise.all([
  loadImages(propertyId),
  loadFloorPlans(propertyId),
  loadPolicies(propertyId),
]);
```

Three reads with no dependency between them now share one round trip of latency. The
same move is wrong for a loop of inserts that reference each other's ids, and it is
wrong for forty reads against a pool of five connections; there, `p-limit` bounds it.
Check the dependency and the pool before converting, and say which case it was.

### 6. Separate the concerns a handler had gathered

Hunt for: a controller that validates, runs SQL, applies a rule and formats the response.

```ts
// Before (controller)
export async function updatePolicy(req: Request, res: Response) {
  const body = PolicySchema.safeParse(req.body);
  if (!body.success) return res.status(400).json({ error: body.error.issues });
  const [existing] = await db.query('SELECT ... WHERE id = ?', [req.params.id]);
  if (existing.source === 'smart' && !existing.sourceLocked) {
    return res.status(409).json({ error: 'policy is managed by the feed' });
  }
  await db.query('UPDATE property_policies SET ...', [...]);
  await db.query('INSERT INTO property_policies_activity_logs ...', [...]);
  res.json({ ok: true });
}

// After
// controller: HTTP in, HTTP out
export const updatePolicy = handler(async (req) => {
  const input = parseBody(req, PolicySchema);
  const policyId = parseId(req.params.id);
  return policyService.update(policyId, input, actorFrom(req));
});

// service: the rule and the transaction
async function update(policyId: PolicyId, input: PolicyInput, actor: Actor): Promise<Policy> {
  return withTransaction(async (trx) => {
    const existing = await policyRepository.loadForUpdate(trx, policyId);
    assertEditable(existing);
    const updated = await policyRepository.update(trx, policyId, input);
    await policyRepository.logActivity(trx, { policyId, actor, before: existing, after: updated });
    return updated;
  });
}

function assertEditable(policy: Policy): void {
  if (policy.source === 'smart' && !policy.sourceLocked) {
    throw new ConflictError('policy is managed by the feed');
  }
}
```

Each piece now has one reason to change. The rule has a name and a unit test with no
database. The two writes share a transaction, which the original did not; that is a
behaviour change, and it is the headline of the report. The error handler turns
`ConflictError` into 409 for every controller at once.

### 7. Two functions instead of a flag; a union instead of a string

Hunt for: `doThing(x, true)`; a parameter typed `string` that only ever holds four values.

```ts
// Before
function createProperty(input: PropertyInput, multiUnit: boolean, status: string) { ... }

// After
const PropertyStatus = {
  Active: 'active',
  Inactive: 'inactive',
  UnderConstruction: 'under_construction',
  PendingApproval: 'pending_approval',
} as const;
type PropertyStatus = (typeof PropertyStatus)[keyof typeof PropertyStatus];

function createSingleUnitProperty(input: PropertyInput, status: PropertyStatus) { ... }
function createMultiUnitProperty(input: PropertyInput, status: PropertyStatus) { ... }

function labelFor(status: PropertyStatus): string {
  switch (status) {
    case PropertyStatus.Active: return 'Active';
    case PropertyStatus.Inactive: return 'Inactive';
    case PropertyStatus.UnderConstruction: return 'Under construction';
    case PropertyStatus.PendingApproval: return 'Pending approval';
    default:
      status satisfies never;
      throw new Error(`unhandled status ${String(status)}`);
  }
}
```

The call site now says what it does. The union makes a typo a compile error, and the
`never` check means adding a fifth status lists every switch that must learn about it.

### 8. Inject the dependency instead of mocking the module

Hunt for: `vi.mock('../src/repositories/...')` or `jest.mock` of anything under `src/`.

```ts
// Before (test)
vi.mock('../src/repositories/prompt.repository', () => ({
  loadActivePrompt: vi.fn().mockResolvedValue({ text: 'v7' }),
}));

// After (code)
export function createScreeningService(deps: {
  loadActivePrompt: () => Promise<Prompt>;
  extract: Extractor;
  clock: () => Date;
}) { ... }

// After (test)
const service = createScreeningService({
  loadActivePrompt: async () => ({ text: 'v7', version: 7 }),
  extract: fakeExtractor,
  clock: () => new Date('2026-01-01'),
});
```

The module mock coupled the test to an import path and to the internal fact that the
service calls that function. The factory makes the dependency visible in the signature,
the test passes a two-line fake, and the service is wired to the real repository in the
composition root with no framework.

### 9. One schema, one parse helper

Hunt for: the same `safeParse` then `res.status(400).json({ error: ... })` block in
every handler.

```ts
// Before: eight copies of
const parsed = Schema.safeParse(req.body);
if (!parsed.success) {
  return res.status(400).json({ error: 'Invalid request body', details: parsed.error.issues });
}

// After
export function parseBody<T>(req: Request, schema: ZodType<T>): T {
  const parsed = schema.safeParse(req.body);
  if (!parsed.success) {
    throw new ValidationError('Invalid request body', parsed.error.issues);
  }
  return parsed.data;
}
```

The eight copies encoded one rule: invalid bodies are 400 with the issues attached. The
rule has one home; the error handler maps `ValidationError` to 400, so the controllers
lose the status code too.

### 10. One level of abstraction

Hunt for: a function whose body is phases separated by blank lines or `// Step 2`.

```ts
// Before: 70 lines, four phases, three levels of nesting

// After
export async function ingestScreeningDocument(upload: Upload, actor: Actor): Promise<Screening> {
  const text = await extractText(upload);
  const prompt = await loadActivePrompt();
  const extraction = await extractCriteria(text, prompt);
  return persistScreening({ upload, extraction, prompt, actor });
}
```

The orchestrator reads as the list of steps a reviewer would write on a whiteboard. Each
step is a function with a name, its own test, and a body short enough to hold in mind.
Nothing about the behaviour changed; the phases already existed, they just had no names.

### 11. Let errors travel

Hunt for: `.then().catch(err => logger.error(err))`, or `try { } catch (e) {
logger.error(e); throw e; }` at every layer.

```ts
// Before
repository.save(row)
  .then(() => res.json({ ok: true }))
  .catch((err) => {
    logger.error(err);
    res.status(500).json({ error: 'failed' });
  });

// After
await repository.save(row);
return { ok: true };
// ... and in the error handler, once:
logger.error({ err, requestId }, 'request failed');
res.status(statusFor(err)).type('application/problem+json').json(problemFor(err));
```

The original logged the failure here, and again in the repository, and again in the
global handler: three lines, no request id. Now the error carries its type and `cause`,
travels to the one place that knows how to answer HTTP, and is logged once with context.

### 12. A log line is a record with a message

Hunt for: `console.log`, template strings passed to the logger, `logger.info('x', obj)`
in winston order, lines with no message.

```ts
// Before
console.log(`Extracted ${criteria.length} criteria for property ${propertyId} in ${ms}ms`);
logger.info({ propertyId, count: criteria.length });

// After
logger.info(
  { propertyId, criteriaCount: criteria.length, durationMs },
  'screening extraction completed',
);
```

The first line had the values inside the sentence, so the log platform saw a different
message on every request and could group nothing. The second had no message at all, so
nothing could search for it. The rewrite has a constant message naming the event and the
values as fields to facet on. Logger setup is in `architecture.md`.

### 13. Existence belongs in the query

Hunt for: `(await repo.findAllBy(...)).length > 0`, or fetching rows to `some` over them.

```ts
// Before
const existing = await db.selectFrom('properties').selectAll().where('company_id', '=', companyId).execute();
if (existing.length > 0) { ... }

// After
const exists = await db
  .selectFrom('properties')
  .select(sql<boolean>`true`.as('exists'))
  .where('company_id', '=', companyId)
  .limit(1)
  .executeTakeFirst();
if (exists) { ... }
```

Materialising every row to ask a yes/no question pays for a fetch the database answers
with an index lookup. The same lesson covers counting and sorting: when the source is
SQL, the work goes in SQL.

## Reporting

State what changed and what was proven equal, in plain sentences. When a conversion was
declined, give the specific blocker (sequential writes, distinct logging, a connection
pool of five), not a vague preference. When behaviour differs in any observable way, that
is the headline of the report, not a footnote. Name the tests that cover the change.
