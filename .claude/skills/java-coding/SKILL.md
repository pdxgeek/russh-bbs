---
name: java-coding
description: How to write Java in this codebase - the house standard for any Java work on 17, 21 or 25 with Spring Boot, from a one-line fix to a new module. Use it every time you write, change, review or test Java - services, controllers, repositories, entities, records, config classes, tests - even when the user only says "add a method", "fix this bug", "write the endpoint for X" or "clean this up" and never mentions Java, style or quality. Also use it when reviewing a diff that shows orElse(null), isPresent()/get(), null-check ladders, hand-rolled pairs, field @Autowired, @Transactional on a private method, Lombok @Data on an entity, string-concatenated log calls, catch blocks that swallow, or mocks of JDK types. The refactoring sub-skill inside covers converting existing code safely.
---

# Writing Java

This is how Java gets written here: modern language features used for what they are for,
immutable data by default, Optional and streams with discipline, Spring wired through
constructors, transactions at the use-case boundary, errors that translate once at each
layer, and tests that run without Spring wherever they can. The standard is the same for
a new module and a two-line fix.

The body of this file is the standard in brief. The references carry the depth and the
reasons; open them when the task touches their area.

| When the task involves | Read |
|---|---|
| Any non-trivial method: streams, Optional, records, sealed types, switch, exceptions, null, collections, naming | `references/style.md` |
| Spring wiring, package layout, transactions, config, JPA, error responses, logging, the JDK (jolta), build tooling | `references/architecture.md` |
| Writing or changing tests, deciding what to mock, slices, Testcontainers | `references/testing.md` |
| Changing code that already works: cleanups, "make this a stream", modernising | `references/refactoring.md` (the refactoring sub-skill) |

## How to work

**Before writing.** Read the surrounding class and one neighbour to learn the repo's
conventions: package layout, how beans are wired, which assertion library and mocking
style the tests use, how errors reach the client, whether Lombok is in play. The repo's
convention wins over this document wherever they differ. Check the Java and Spring Boot
versions in the build file before using an API that changed between releases: pattern
matching for switch needs 21, `ScopedValue` is final only from 25, `StructuredTaskScope`
is still preview in 25, and Spring Boot 4 renamed starters and moved Jackson to
`tools.jackson`.

**While writing.** Start from the data: which values are immutable records, which states
are a sealed hierarchy, where null is legitimate. Then write methods that read as a
sentence, each at one level of abstraction. Validate at the boundary, trust inside.
Translate exceptions once per layer. Keep Spring out of the domain so the domain can be
tested with `new`.

**The JDK.** JDKs are managed with jolta. The project's `.java-version` says which JDK
it builds with, the shims pick it up for `java`, `javac` and the build wrapper, and
`jolta exec <cmd>` or `eval "$(jolta env)"` set `JAVA_HOME` where a tool needs it. Never
install a JDK with brew or sdkman, never set `JAVA_HOME` by hand, and never edit
`.java-version` except to deliberately move the project to a new version. If a project
has no `.java-version`, pin one (`jolta pin 21`) and commit it. Details in
`references/architecture.md`.

**Before calling it done.** Compile with the project's checks (Error Prone, NullAway,
Checkstyle where configured), run the tests scoped to the touched code, and run the
formatter. State what was verified and how; when something could not be verified, say so
first.

## The standard in brief

### Data

- Records for every immutable carrier: DTOs, projections, compound keys, multi-value
  returns, events, config properties. Validate and defensively copy (`List.copyOf`) in
  the compact constructor. Never a record for a JPA entity.
- Sealed interface plus records for a closed set of variants (results, commands,
  states), consumed with a pattern `switch` and no `default`, so adding a variant breaks
  every switch that forgot it. Classic polymorphism when the variant set is open.
- Immutable collections from the factories: `List.of`, `Map.of`, `List.copyOf`,
  `Stream.toList()`. `Collections.unmodifiableList` is a view over a list someone else
  can still mutate. The factories reject null elements, and `Set.of`/`Map.of` iterate in
  random order; use `List` or `LinkedHashMap` when order matters.
- Fields `private final`, declared by interface type, initialised once. No setter that
  exists only because a framework might want it.
- `java.time` with an injected `Clock`; never `Date`, `Calendar`, `SimpleDateFormat`, or
  `LocalDateTime` for an instant.

### Null and Optional

- JSpecify: `@NullMarked` in every `package-info.java`, `@Nullable` only where null is
  legitimate, written in type-use position (`public @Nullable Foo find(...)`, not
  `@Nullable public Foo`). Overrides repeat the annotation. `Objects.requireNonNull` at
  constructor and public-method boundaries, then no null checks inside.
- Optional is a return type for "no result", never a field, parameter or collection
  element. Never `get()`; `orElseThrow()` when presence is proven. `orElseGet` when the
  default costs anything. `orElse(null)` only at a boundary handing off to a
  nullable-consuming API. A single-step `ofNullable(x).orElse(y)` is a ternary.
- Internal and private APIs may return `@Nullable T` instead of Optional; pick one per
  layer and keep it.

### Streams, loops and switch

- Streams for uniform work over a sequence; a loop when the body mutates locals, needs
  break or early return, or throws checked exceptions. Convert only when the stream reads
  better. Extract fat lambdas into named private methods.
- Pure functions in pipelines. `forEach` only to present results, never to accumulate;
  use a collector. `anyMatch` over `filter().count() > 0`. `min`/`max` with a composed
  `Comparator` and a unique-key tiebreak over sort-then-first. No `.parallel()` without
  a measurement.
- Push filtering, counting and existence to the database: `existsBy...` over
  `!findAll...().isEmpty()`.
- Switch expressions with `->`, exhaustive, no `default` over enums or sealed types.
  `case null` when the selector can be null; a pattern switch throws on null otherwise.
  Deconstruct records in the case label. Enhanced `instanceof` with the negated-guard
  idiom; an `instanceof` chain over a sealed type is a switch.
- `var` when the initializer states the type or to name the steps of a broken-up chain.
  Not with diamond, bare generic factories or bare numeric literals.

### Methods and names

- One level of abstraction per method; a method either orchestrates named steps or is a
  step. A body that needs section comments is several methods. Guards first, happy path
  unindented, at most two levels of nesting.
- Constructors with more than about five collaborators mean the class has several jobs.
- Google Java Style: 100 columns, braces always, no wildcard imports, `@Override`
  wherever legal, acronyms as words (`HttpClient`, `getXmlUrl`), `UPPER_SNAKE` only for
  deeply immutable constants. Utility classes are `final` with a private constructor and
  a noun name, never `*Util` dumping grounds. No `I` prefix, no `Impl` suffix for a sole
  implementation.
- Comments say why, never what. Javadoc on public API types and members.

### Exceptions

- Unchecked in application code. One `DomainException extends RuntimeException` per
  bounded context, leaf types only when callers branch on them, standard exceptions
  (`IllegalArgumentException`, `IllegalStateException`) otherwise.
- Translate at layer boundaries with the original as `cause`. Never swallow: an empty
  catch carries a comment saying why. Never `e.printStackTrace()`; log with the throwable
  as the last argument. try-with-resources for every `AutoCloseable`, including
  `Files.lines`, `Files.list` and an `ExecutorService`.
- REST errors as RFC 9457 `ProblemDetail` from one `@RestControllerAdvice`. Controllers
  never build error bodies by hand.

### Spring

- Constructor injection, one constructor, no `@Autowired` on it, no field injection.
  Package by feature with package-private internals; modules talk through public beans or
  application events, never another module's repository or entity.
- `@Transactional` on public service methods only. Self-invocation and private methods
  bypass the proxy. Checked exceptions and caught-and-logged exceptions commit. Every
  read-only method says `readOnly = true`. `spring.jpa.open-in-view=false`.
- `@ConfigurationProperties` records with `@Validated`, `Duration` and `DataSize` types.
  No `@Value` scattered through services.
- Entities never leave the service layer; map to records. Every association `LAZY`,
  fetched per use case with `JOIN FETCH`, `@EntityGraph` or a projection. `SEQUENCE`
  ids, never `IDENTITY` on Postgres. No Lombok `@Data`, `@EqualsAndHashCode` or
  `@ToString` on an entity.
- Virtual threads for I/O-bound request handling (`spring.threads.virtual.enabled`);
  never pool them; bound downstream concurrency with a `Semaphore` and size the
  connection pool for the database, which becomes the choke point.

### Logging

SLF4J with `{}` placeholders, never concatenation. Throwable last. ERROR means someone
acts now, WARN means handled but abnormal, INFO means a business lifecycle event, one or
two per request. Log once at the boundary that handles the failure. Never log secrets,
tokens, headers or full bodies; a record's generated `toString` includes every
component, so keep secrets out of records that get logged. Structured JSON through
Boot's `logging.structured.format`; MDC for request-scoped context, cleared in `finally`.

### Tests

JUnit 5 with package-private classes and `@Nested` groups; AssertJ everywhere;
`@ParameterizedTest` over copy-pasted cases. Plain JUnit for services (no Spring),
`@WebMvcTest` for controllers, `@DataJpaTest` with Testcontainers and `Replace.NONE` for
repositories, a few `@SpringBootTest` end to end. Mockito strict stubs; do not mock types
you do not own or build data with mocks; call the constructor rather than `@InjectMocks`.
`@MockitoBean`, not the removed `@MockBean`. Details in `references/testing.md`.

## Refactoring existing code

Changing code that already works is its own discipline: equivalence first, delete before
converting, one move per smell, verified with the project's tests. The smell-to-move table
and the worked examples live in `references/refactoring.md`. Read it whenever the task is
a cleanup, a modernisation or a review of someone else's diff.

## Reporting the work

State what was built or changed and what was verified, in plain sentences. Name the tests
that cover it. When a decision went against this standard because the repo does it
differently, say so in one line. When behaviour changed in any observable way, that is the
headline, not a footnote.
