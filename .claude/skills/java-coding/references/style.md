# Style: the language, used well

The reasons behind the rules in SKILL.md, with the details that decide edge cases.
Sources: Effective Java 3rd ed. (Bloch; item numbers below), Stuart Marks' "Optional:
The Mother of All Bikesheds" and the OpenJDK var style guide, Brian Goetz on records and
data-oriented programming, JEPs 361, 378, 394, 441, 444, 491, 505, 506, the Oracle Java
21 language guide, the Spring Framework 7 null-safety reference and JSpecify user guide,
the Google Java Style Guide, Error Prone bug patterns, Vlad Mihalcea on JPA performance.

Contents: Version notes · Records · Sealed types and pattern matching · Switch,
instanceof and text blocks · Immutability and collections · Optional · Null handling
with JSpecify · Streams · Comparators · var · Exceptions · Naming and structure ·
Concurrency and virtual threads · Performance pitfalls

## Version notes (check the build file before using an API)

- Java 17: records, sealed types, text blocks, switch expressions, enhanced `instanceof`.
- Java 21: pattern matching for switch and record patterns (final), virtual threads
  (final), sequenced collections. Unnamed `_` is preview; name the component.
- Java 22: unnamed variables and patterns `_` final.
- Java 24: `synchronized` no longer pins virtual threads (JEP 491).
- Java 25 (current LTS): `ScopedValue` final (JEP 506). `StructuredTaskScope` is still
  preview (JEP 505, API changed to `StructuredTaskScope.open(Joiner)`); primitive
  patterns still preview. Nothing preview goes in production code.
- Spring Boot 4 / Framework 7 (current): JSpecify annotations throughout, Spring's own
  `org.springframework.lang` nullability annotations deprecated, `@MockBean` removed,
  starters and Jackson packages renamed. See `architecture.md`. Boot 3.5 open-source
  support ended June 2026.

## Records

Intent (Goetz): transparent carriers for immutable data; the API is the state. Derived
`equals`, `hashCode` and `toString` cover exactly the declared components.

- Reach for a record for compound keys, multi-value returns, DTOs, messages, events and
  intermediate results. Keep helper records private and nested, or local to a method,
  when only one class cares; they are cheap enough to be single-use.
- Validate in the compact constructor; normalise by reassigning the constructor
  parameters there. All non-canonical constructors funnel through the canonical one.
- Records are shallowly immutable: defensively copy mutable components
  (`this.items = List.copyOf(items)`) or the equals and map-key contract breaks.
  SpotBugs `EI_EXPOSE_REP` on a record is fixed this way, never suppressed.
- Local records inside a method name the intermediate result of a pipeline: compute a
  derived value, sort on it, project it away. This is the Oracle-documented idiom.
- Behaviour on a record is limited to derived quantities; business logic belongs in
  services or in the switch at the point of use.
- Not for JPA entities (they need a no-arg constructor, mutability and proxy identity).
  Excellent as JPA projections and as `@ConfigurationProperties`. Jackson handles
  records; Java serialisation deserialises through the canonical constructor, so the
  validation runs.
- The generated `toString` includes every component. A record holding a secret, token
  or password must not be logged, or must hold the secret in a wrapper whose `toString`
  masks it.

## Sealed types and pattern matching

- `sealed interface` plus `record` implementations for a closed set of variants:
  results, commands, events, states. The compiler enumerates the cases and a new variant
  breaks every switch that forgot it. Classic interface-plus-implementations when the
  variant set is open or owned by other modules. Goetz frames them as complementary
  tools for different granularities.
- Each permitted subtype must be `final`, `sealed` or `non-sealed`; a record is
  implicitly final. Declare `permits` when the subtypes are not in the same file.
  Omitting the modifier is a compile error that generated code commits often.
- No `default` on a switch over a sealed type or an enum. It silently absorbs a new
  subtype and defeats the exhaustiveness check. Google style asks for exhaustiveness
  "even where the language does not require it"; for pattern switches the compiler
  already checks it, so the Java team's advice wins. Error Prone's `MissingDefault`
  conflicts with this rule and stays off.
- A pattern switch throws `NullPointerException` on a null selector unless there is a
  `case null` (or `case null, default`). `instanceof` is null-friendly and returns false.
  `case null, String s` does not compile; null combines only with `default`.
- Label order: constants first, guarded patterns (`when`) next, unguarded type patterns
  last, subtypes before supertypes. A dominated label is a compile error.
- Deconstruct records in the case label (`case Point(int x, int y)`) rather than binding
  and calling accessors; nest patterns for nested records. The compiler verifies
  exhaustiveness through nested record patterns; accessor chains get no such check. `var`
  inside a record pattern for components you do not care about.
- Pattern switches use `->` only; fall-through into a pattern label does not compile.
- Separate compilation: an exhaustive switch compiled against an older sealed hierarchy
  throws `MatchException` when the hierarchy grows and the switch is not recompiled.
  Relevant in multi-module and plugin builds.

## Switch, instanceof and text blocks

- Switch expressions with `->`. A block arm needs `yield`, never `return`; `return`
  inside a switch expression does not compile. The colon form allows fall-through and
  is for legacy code only.
- A switch expression over an enum is exhaustive without `default` and stays that way.
  A switch statement over an enum is not required to be exhaustive and silently does
  nothing for a new constant; convert it to an expression or add an explicit `throw`.
  Error Prone `MissingCasesInEnumSwitch` catches the gap.
- Enhanced `instanceof` with the negated guard: `if (!(o instanceof Foo foo)) return;`
  and `foo` is in scope afterwards. Flow scoping puts the binding wherever the match is
  definitely true; re-casting after the check is the tell of code that has not learned
  this.
- A pattern variable cannot shadow a local or field in scope. Pick a distinct name.
- Text blocks for SQL, JSON and HTML literals, with `.formatted(...)` for values that
  are not user input. The closing `"""` sets the left margin: on its own line aligned
  with the content to strip common indentation, at the end of the last line to drop the
  trailing newline. Trailing spaces are stripped; `\s` keeps one, `\<newline>` joins
  lines. Never mix tabs and spaces in the indentation; enable `-Xlint:text-blocks`.

## Immutability and collections

- Return and store `List.of`, `Set.of`, `Map.of`, `List.copyOf`, `Stream.toList()`.
  `Collections.unmodifiableList(list)` is a view; whoever holds the backing list can
  mutate what callers see. `List.copyOf` returns the same instance when the source is
  already unmodifiable, so defensive copying is cheap.
- The JDK immutable collections reject null elements, keys and values. `Set.of` and
  `Map.of` reject duplicates. `Set.copyOf` and `Collectors.toUnmodifiableSet` keep an
  arbitrary duplicate; `Collectors.toUnmodifiableMap` throws on duplicate keys without
  a merge function. These are the runtime failures that appear when `Arrays.asList` or
  `new HashSet<>(...)` code is converted.
- `Set.of` and `Map.of` iteration order is unspecified and randomised per JVM run. Tests
  that print or compare order go flaky; use `List` or `LinkedHashMap` when order matters.
- `Stream.toList()` is unmodifiable and null-tolerant; `Collectors.toList()` is an
  `ArrayList` in practice; `Collectors.toUnmodifiableList()` rejects nulls. Switching to
  `toList()` breaks a caller that adds to the result, which makes it a behaviour change.
- Defensive copy on the way in (constructor, compact constructor); no copy on the way
  out when the field is already immutable (item 50).
- Fields holding collections: interface type, initialised once, `final`, never exposed
  mutable through a getter.
- `Arrays.asList` is fixed-size but element-mutable and null-tolerant. Use `List.of`
  when it fits. `Collections.emptyList()` and `singletonList` are `List.of()` and
  `List.of(x)` in new code.
- Guava in a Guava codebase is fine: `ImmutableList` documents immutability in the
  signature, has builders, and iterates in insertion order. In a codebase without Guava,
  the JDK factories.
- Enums over int or String constants; `EnumSet` and `EnumMap` for enum-keyed collections.

## Optional

Design intent (Marks, Goetz): a return-type mechanism for "no result" where null causes
errors. Eliminating every null is explicitly not the goal.

**The Marks rules**
1. Never use null for an Optional variable or return value.
2. Never call `get()`; `orElseThrow()` when presence is proven, so the failure mode is
   explicit.
3. Prefer the chaining API over `isPresent()` plus `get()`, which is a null check in a
   costume.
4. Do not create an Optional to chain one step to a value:
   `Optional.ofNullable(s).orElseGet(this::getDefault)` is a ternary with an
   allocation. Multi-step chains doing real work are the intended style; the rule bans
   the single-step wrap-then-unwrap.
5. A nested chain or an `Optional<Optional<T>>` intermediate means it is too complex;
   unnest or use an if.
6. No Optional fields, parameters, or collection elements. Fields cost an allocation and
   a dependent load; parameters force every call site to wrap; return the empty
   collection instead of `Optional<List<T>>`.
7. No identity-sensitive operations (`==`, locking) on Optionals; it is a value-based
   class.

**API patterns**
- `map`, `filter`, `flatMap` off an Optional you were given; `or(() -> fallback)` for a
  lazy alternative Optional, which can carry a branch-specific side effect such as a warn
  log; `ifPresent` and `ifPresentOrElse` for actions.
- `orElse(constant)` only for cheap constants; `orElseGet(supplier)` when the default is
  computed, because `orElse`'s argument is evaluated even when the value is present.
- `orElseThrow(MyException::new)` with a factory, so nothing is built on the happy path.
- A stream of Optionals flattens with `.flatMap(Optional::stream)`.
- Boundaries: nullable in with `Optional.ofNullable(x)`; nullable out with
  `orElse(null)`, the only sanctioned use of `orElse(null)`.
- `map` turns a null mapper result into empty; do not pre-guard for it.
- Return `Optional<T>` when absence is a normal case the caller must handle (item 55).
  Never wrap containers. `OptionalInt`, `OptionalLong`, `OptionalDouble` for primitives.
  Not as map keys or values.
- Spring's position (Deleuze) is that internal and private APIs may return `@Nullable T`
  instead; Optional costs an allocation and API churn. Pick one convention per layer.

## Null handling with JSpecify

- JSpecify (`org.jspecify.annotations`) is the annotation set Spring 7, Boot 4, Spring
  Data 4, Security 7 and Kotlin 2 understand. Not `javax.annotation`, `org.jetbrains`,
  `org.checkerframework`, or Spring's own `org.springframework.lang.*` (deprecated).
- `@NullMarked` in `package-info.java` for every package. Non-null becomes the default;
  `@Nullable` marks the exceptions. `@NonNull` is almost never written.
- `@Nullable` is a type-use annotation: `private @Nullable String name;`,
  `public @Nullable Foo find(...)`, `List<@Nullable String>`, `java.lang.@Nullable
  String`. For arrays, `@Nullable Object[]` means nullable elements and `Object
  @Nullable []` means a nullable array. `@Nullable public String foo()` is the
  declaration-position mistake; google-java-format's `formatAnnotations` fixes placement.
- Annotations are not inherited. Repeat `@Nullable` on overriding methods and
  parameters, or the override silently becomes non-null.
- `Objects.requireNonNull(x, "x")` or Spring `Assert.notNull` at constructor and
  public-method boundaries (item 49, fail fast), then rely on `@NullMarked` inside. No
  `if (x != null)` sprinkled through code that is non-null by contract.
- NullAway excludes `@Autowired`, `@Inject`, `@Mock` and `@TempDir` fields and treats
  `@PostConstruct` and `@BeforeEach` as initialisers. Anything else late-initialised
  gets `@SuppressWarnings("NullAway.Init") // reason`. Constructor injection makes the
  question disappear.
- Suppress only with a reason comment, never blanket.
- Spring 7 and Boot 4 return `@Nullable` from many APIs (`getHeader`, `RestClient`
  bodies). Dereferencing without a check is now a NullAway error; add the check instead
  of assuming non-null as Boot 3 code did.

## Streams

**When streams, when loops (item 45)**
- Streams for uniform work over sequences: map, filter, accumulate, group, search.
- A loop when the body must mutate locals, break, continue or return the enclosing flow,
  or throw checked exceptions; lambdas can do none of these.
- Once you `map`, the original element is gone; when a later stage needs both, carry the
  pair in a local record or keep the loop.
- Convert a loop only when the stream version is clearer. When in doubt, write both
  shapes mentally and pick the more readable one.
- Extract multi-line lambda bodies into named helper methods. Names matter more in a
  pipeline because there are no intermediate variable names to carry meaning. A callee
  that throws a checked exception goes behind a helper that translates it into the
  domain's runtime exception.

**Purity and forEach (item 46)**
- Every function passed to a stream operation is pure. Stateful lambdas defeat
  correctness and readability.
- `forEach` that does more than present already-computed results is a smell; the classic
  misuse is accumulating into a collection inside `forEach` instead of using a collector.
- `anyMatch`, `allMatch`, `noneMatch` over `filter(...).count() > 0` or
  `findFirst().isPresent()`; they short-circuit and state intent.
- When the source is a database, push filtering and existence down: a derived
  `existsBy...` repository method beats fetching rows to test emptiness.

**Collectors**
- `toMap` with a merge function, `groupingBy` with downstream collectors, `joining`,
  `teeing` for two aggregates in one pass.
- `stream.toList()` over `collect(Collectors.toList())`, noting the mutability change;
  `Collectors.toCollection(ArrayList::new)` when mutability is required.
- `mapMulti` over `flatMap` when each element expands to a few, possibly zero, elements
  and building a stream per element is waste.

**Parallel (item 48)**
- Never `.parallel()` by default. Only with a measured speedup, a splittable source
  (ArrayList, arrays, ranges), a cheap reduction terminal and pure functions. Never with
  `Stream.iterate` or `limit`. All parallel streams share one ForkJoinPool.

## Comparators

- Compose declaratively: `comparing(...).thenComparing(...).reversed()`, with
  `comparingInt` and `comparingLong` to avoid boxing. Add a final unique key
  (`thenComparing(User::getId)`) when a pick must be deterministic. Wrap with
  `Comparator.nullsFirst` or `nullsLast` instead of null-checking inside the lambda.
- `min` and `max` with a comparator replace sort-then-take-first and never throw on
  empty.

## var

The OpenJDK style guide's principles: readers over writers; the declaration must be
understandable from local context without an IDE; the explicit type is one information
channel and may be muted only when the name and initializer carry the load.

- Use `var` when the initializer states the type: constructor calls, well-named factories
  (`var reader = Files.newBufferedReader(path)`), literals.
- Use `var` to break a mega-chain into named intermediate steps; unpronounceable generic
  types (`Optional<Map.Entry<String, Long>>`) are what push people into unreadable
  one-liners.
- Pair `var` with a better variable name; the name does the work the type used to.
- Keep scopes small first; `var` in a long scope hides a changed initializer type.
- Diamond and generic methods: `var q = new PriorityQueue<>()` infers `Object`; supply
  the type argument or skip `var`.
- Numeric literals: `var flags = 0` is int; the silent widening an explicit type
  performed disappears.
- Locals only; fields, parameters and returns keep explicit types, where programming to
  the interface still applies.
- `final` on locals and parameters is not added by default; effectively-final is
  inferred for lambdas and `var` has made the noise argument win. `final` on fields,
  always, unless there is a documented reason for mutability.

## Exceptions

- Unchecked by default in application code (item 71). Checked exceptions do not compose
  with lambdas and do not roll back Spring transactions. Checked only when the caller
  can realistically recover and the decision must be forced, which is rare in services.
- A small hierarchy: one abstract `DomainException extends RuntimeException` per bounded
  context, leaf types (`OrderNotFoundException`) only when callers branch on them,
  standard exceptions otherwise (item 72). A class per message is noise.
- Every custom exception has `(String message)` and `(String message, Throwable cause)`
  constructors and carries the failure-capture data (id, state) in the message or as
  accessors (item 75). No secrets or personal data in messages; they end up in logs and
  HTTP bodies.
- Translate at layer boundaries (item 73): catch `IOException`, `DataAccessException`
  or a client exception at the edge of the layer that understands it and rethrow a
  domain exception with the original as `cause`. Callers must not depend on the
  persistence technology.
- Never swallow (item 77). An empty catch is legal only with a comment saying why.
  `catch (Exception e) { log.error(...); }` and continuing inside a `@Transactional`
  method commits partial work.
- Never `e.printStackTrace()`. Log with the exception as the last argument:
  `log.error("Failed to save order {}", id, e)`. SLF4J prints the stack trace only when
  the throwable is the final argument. Error Prone `CatchAndPrintStackTrace` flags it.
- try-with-resources for every `AutoCloseable`: streams, `Connection`, `Statement`,
  `ExecutorService` (19+), `Files.lines`, `Files.list`. The last two hold an open file
  handle and are the ones generated code forgets. Several resources go in one header;
  suppressed exceptions are preserved.
- Exceptions are not control flow (item 69). For expected outcomes such as "not found"
  with an internal caller, return `Optional` or a sealed result type; at a REST boundary
  throw and let the advice map it to 404. Pick one per codebase.
- Spring's data layer throws `DataAccessException` subtypes; translation is automatic
  for `@Repository` beans and Spring Data repositories. Catch
  `DataIntegrityViolationException` or `OptimisticLockingFailureException` where you can
  act (retry, map to 409); never catch `SQLException` or `PersistenceException` directly.

## Naming and structure

- Google Java Style essentials: no wildcard imports; static imports only for static
  members, never nested classes; ASCII-sorted import groups (static block, blank line,
  non-static block); 100 columns; braces always, K&R; one declaration per statement;
  locals declared at first use; `String[] args`; `@Override` whenever legal; modifiers
  in JLS order; `long` literals with `L`; no finalizers. google-java-format and
  Checkstyle enforce most of it mechanically.
- Acronyms are words: `HttpClient`, `getXmlUrl`, `supportsIpv6`. Error Prone
  `InconsistentCapitalization` catches field and parameter mismatch.
- `UPPER_SNAKE_CASE` only for `static final` and deeply immutable constants. A `static
  final List` that is mutable, or a `final` local, stays camelCase. This is the most
  common naming error in generated code.
- Static imports: fine for test DSLs (`assertThat`, `when`, `given`, `Collectors.*`,
  `Map.entry`); avoid for production constants from other classes unless the class is
  obvious (`Math.max`); never for a method whose name is ambiguous out of context.
- Method length: one screen, around twenty to thirty lines, one level of abstraction;
  extract when a comment is needed to label a block. A service with more than about five
  dependencies or three to four hundred lines is a candidate to split by use case.
- Utility classes are `final` with a `private` constructor (item 4), hold only static
  members, and are named by noun (`Durations`, `JsonCodec`). `*Util` and `*Helper` grow
  unbounded. When state or substitution is needed, a record, a sealed type or a bean.
- Javadoc on every public type and member of an API package; skip trivial getters and
  overrides. Summary fragment is a noun or verb phrase; tags in `@param`, `@return`,
  `@throws`, `@deprecated` order. Package-private code needs comments only for
  non-obvious intent.
- No `I` prefix on interfaces. No `Impl` suffix when there is one implementation; and
  then no interface at all unless a boundary or test seam needs it. Spring's own
  convention for the rare second implementation is `Default*` or `Simple*`.
- Comments say why, never what. A name that needs a comment is the wrong name.

## Concurrency and virtual threads

- Virtual threads for I/O-bound, high-concurrency, thread-per-request code (JEP 444).
  Throughput comes from unmounting during blocking; they do not speed up CPU-bound work.
- Never pool virtual threads. `Executors.newVirtualThreadPerTaskExecutor()` in
  try-with-resources, or `Thread.ofVirtual()`. To cap access to a scarce resource, a
  `Semaphore`, never a fixed pool.
- No expensive objects cached in `ThreadLocal` on virtual threads; millions of threads
  means millions of copies never reused. `ScopedValue` (final in 25) for per-request
  context. Audit with `-Djdk.traceVirtualThreadLocals=true`.
- Pinning: on Java 21 to 23, blocking inside `synchronized` pins the carrier, so hot
  `synchronized` blocks that guard I/O become `ReentrantLock`. On Java 24+ (JEP 491)
  `synchronized` no longer pins; do not rewrite working code. Native frames and class
  initialisation still pin. Observe with JFR `jdk.VirtualThreadPinned`.
- Lock scopes narrow; never I/O while holding a lock, whatever the thread type.
- Bound downstream concurrency explicitly. Unlimited virtual threads against a
  ten-connection pool means timeouts, not throughput; size the pool for the database.
- `StructuredTaskScope` is preview through 25. On 21 use `CompletableFuture` or
  `ExecutorService.invokeAll` with a virtual-thread executor. When preview is enabled:
  fork all, `join()` once, never reuse a `Joiner`.
- `@Async` and `@Transactional` on the same method does not get the caller's
  transaction; the transaction is thread-bound. Prefer a virtual-thread executor and
  plain blocking calls over `@Async` fan-out.

## Performance pitfalls

- String concatenation in a single expression is fine and often faster than a hand-rolled
  `StringBuilder` (JEP 280 `invokedynamic`). `s += x` in a loop is quadratic;
  `StringBuilder`, `String.join`, `Collectors.joining` or `String.repeat` there.
  Rewriting single expressions into `StringBuilder` is noise.
- Boxing (item 61): never `Long sum = 0L` accumulators or `Integer` loop counters; never
  `==` on boxed values (the `Integer` cache covers -128 to 127, so
  `Integer.valueOf(1000) == Integer.valueOf(1000)` is false). `long`, `int`,
  `IntStream`, `LongStream` for numeric pipelines. Error Prone `ReferenceEquality` and
  `BoxedPrimitiveEquality` catch the comparisons.
- `java.time` with an explicit `ZoneId` or an injected `Clock`. Error Prone
  `JavaTimeDefaultTimeZone` flags system-default zone usage.
- `Pattern`, `DateTimeFormatter`, `ObjectMapper` and `JsonMapper` are thread-safe and
  belong in `static final` fields (item 6). `String.split`, `matches` and `replaceAll`
  recompile the pattern every call. `SimpleDateFormat` is not thread-safe and is not
  used.
- Initial capacity on `HashMap` and `ArrayList` only when the size is known and large.
- JPA pitfalls (N+1, `IDENTITY`, entity `equals`, open-in-view) are in
  `architecture.md`; they are where most Java performance bugs actually live.
