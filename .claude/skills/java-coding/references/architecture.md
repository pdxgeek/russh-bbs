# Architecture: Spring Boot structure, transactions, JPA, errors, logging, tooling

How a Spring Boot service is put together here. Sources: the Spring Framework reference
(dependency injection, transaction management, null safety, error responses), the Spring
Boot reference (externalized configuration, task execution, logging, testing), the
Spring Boot 4.0 migration guide, Spring Modulith, Vlad Mihalcea and Thorben Janssen on
JPA, the SLF4J manual, the OWASP logging cheat sheet, Error Prone, NullAway, Spotless,
Checkstyle, SpotBugs and JaCoCo documentation.

Contents: Version notes · Wiring · Package layout · Transactions · Configuration · JPA
and persistence · Error responses · Logging · Virtual threads in Boot · JDK management
with jolta · Build and tooling

## Version notes

Spring Boot 4 and Framework 7 are current; Boot 3.5 open-source support ended June 2026.
Renames that generated code gets wrong on Boot 4:

- `spring-boot-starter-web` is `spring-boot-starter-webmvc`.
- Jackson 3 lives in `tools.jackson.*`; annotations stay `com.fasterxml.jackson.annotation`.
  A custom mapper bean is a `JsonMapper`, not an `ObjectMapper`. `@JsonComponent` is
  `@JacksonComponent`.
- `@MockBean` and `@SpyBean` are removed; `@MockitoBean` and `@MockitoSpyBean`.
- `@SpringBootTest` no longer auto-configures `MockMvc` or `TestRestTemplate`; add
  `@AutoConfigureMockMvc` or use `RestTestClient`.
- `spring.dao.exceptiontranslation.enabled` is
  `spring.persistence.exceptiontranslation.enabled`. Undertow is gone.
- Spring's `org.springframework.lang` nullability annotations are deprecated in favour
  of JSpecify (see `style.md`).

Java 17 remains the minimum for Boot 4; Spring recommends the latest LTS.

## Wiring

- Constructor injection only. One constructor, no `@Autowired` on it (Spring detects a
  single constructor), fields `private final`. The object is immutable and fully
  initialised, and a plain-Java unit test needs no Spring. Field `@Autowired` hides
  dependencies and prevents `final`. Setter injection only for a genuinely optional
  dependency with a default.
- A constructor with more than about five collaborators is a smell; split the service by
  use case. Large services also make the `@Transactional` self-invocation bug more
  likely.
- Lombok `@RequiredArgsConstructor` is acceptable for the constructor, never combined
  with field `@Autowired`. Some teams ban Lombok entirely; records and `var` have
  reduced the need. Follow the repo.
- Services depend on interfaces only where there is a real boundary or a test seam. One
  implementation means no interface and no `Impl` suffix.
- Domain code (records, sealed types, pure rules) has no Spring imports, so it is tested
  with `new`.

## Package layout

- Package by feature, not by layer: `com.acme.order`, `com.acme.inventory`, each with
  package-private internals and a small public API of beans and events. Layer packages
  (`controller`, `service`, `repository`) force everything `public` and hide ownership;
  feature packages let the compiler enforce encapsulation.
- Spring Modulith's convention: direct sub-packages of the application class are
  modules; an `internal` sub-package is hidden. Enforce with
  `ApplicationModules.of(App.class).verify()` in a test, or ArchUnit rules (see
  `testing.md`).
- Modules talk through public beans or application events (`ApplicationEventPublisher`,
  `@ApplicationModuleListener`), never through another module's repository or entity.
- Inside a feature: a thin controller, a service holding the use cases and the
  transaction, repositories, entities and mappers. Controllers validate (`@Valid`),
  delegate and map; they hold no business logic and call no repository.
- Entities never leave the service layer. Request and response types are records; Spring
  Data interface or record projections for read models. Exposed entities leak schema,
  trigger lazy loading during serialisation, and recurse on bidirectional references.

## Transactions

- `@Transactional` on service methods, the use-case boundary. Not on controllers (they
  would hold a connection during serialisation), not on repositories (Spring Data wraps
  them already).
- Self-invocation bypasses the proxy: calling `this.saveAll()` from a non-transactional
  method in the same class starts no transaction, and `@Transactional` on a `private`
  method is ignored. Fix by moving the method to another bean, never by self-injection.
  Since Spring 6.0 `protected` and package-private methods are proxied with class-based
  proxies; interface proxies still need `public`. Keep transactional methods public.
- Checked exceptions do not roll back by default; only `RuntimeException` and `Error`
  do. Either never throw checked exceptions from transactional code, set
  `@Transactional(rollbackFor = Exception.class)`, or (Spring 6.2+)
  `@EnableTransactionManagement(rollbackOn = RollbackOn.ALL_EXCEPTIONS)` globally.
- Catching an exception inside a transactional method and continuing commits the
  transaction. If an inner `REQUIRES_NEW` or a nested transactional call marked
  rollback-only, the outer commit throws `UnexpectedRollbackException`. Both are
  frequent "why did it commit" and "why did it blow up at commit" bugs.
- Annotate concrete classes, not interfaces; annotations are not inherited. No
  `@Transactional` in `@PostConstruct` or constructors; the proxy is not in place yet.
- `@Transactional(readOnly = true)` on every read-only service method. Hibernate sets
  `FlushMode.MANUAL` and a read-only session (no dirty-checking snapshots, less memory),
  and routing data sources can send the query to a replica. Entities modified in such a
  method are not flushed.
- `spring.jpa.open-in-view=false`, explicitly. Open Session in View holds a connection
  for the whole request, runs lazy loads in auto-commit outside the transaction, and
  masks `LazyInitializationException` and N+1 instead of fixing the fetch plan.

## Configuration

- `@ConfigurationProperties` as a `record`: constructor binding is implicit for records,
  `@DefaultValue` for defaults, `@Validated` plus Bean Validation constraints, registered
  with `@ConfigurationPropertiesScan` or `@EnableConfigurationProperties`. A properties
  record is not a `@Component` and injects no beans.
- Defaults declared in the record are not visible in the `Environment`, so
  `@ConditionalOnProperty` and `@Value` placeholders do not see them.
- `Duration` and `DataSize` for timeouts and sizes; Boot converts `30s` and `10MB`.
- No `@Value("${...}")` scattered through services. One typed properties record per
  concern, injected where needed.
- Secrets from the environment or a secrets manager, never from code, and never in a
  record that gets logged.

## JPA and persistence

Most Java performance bugs live here.

- Every association `fetch = FetchType.LAZY`. The defaults are LAZY for collections and
  EAGER for `@ManyToOne` and `@OneToOne`; EAGER cannot be turned off per query, LAZY can
  be turned on. Fetch per use case with `JOIN FETCH`, `@EntityGraph` or a DTO projection.
- Detect N+1 in tests by asserting statement counts (Hibernate `Statistics`,
  datasource-proxy, or `hibernate.generate_statistics=true` in the test profile).
  `@BatchSize` and `default_batch_fetch_size` mitigate; they are not the design.
- Pagination plus `JOIN FETCH` on a collection pages in memory (warning `HHH90003004`),
  and two `List` fetches raise `MultipleBagFetchException`. Page ids first and then
  fetch, fetch one collection at a time, or use `Set` for one of them.
- Ids: `GenerationType.SEQUENCE` with `allocationSize` 20 to 50, plus
  `hibernate.jdbc.batch_size=25` and `order_inserts`/`order_updates=true`. `IDENTITY`
  disables JDBC batching because Hibernate needs the id before persisting; acceptable on
  MySQL only.
- `saveAll` in a loop without batching is one round trip per row; `deleteAll(entities)`
  loads then deletes one by one. `deleteAllInBatch` or a modifying JPQL query for bulk
  work.
- Entity `equals` and `hashCode`: never Lombok `@Data`, `@EqualsAndHashCode` or
  `@ToString` on an entity (every field, lazy associations, recursion, stack overflow).
  Use a natural key when one exists; otherwise `equals` compares ids only when both are
  non-null (same class, or `Hibernate.getClass` for proxies) and `hashCode` returns a
  constant, so the value is stable from transient to managed to detached. `toString`
  lists basic attributes only. Lombok `@Getter`, `@Setter` and `@Builder` are fine.
- Three modelling mistakes to reject in review: `Optional` entity fields (Hibernate
  cannot map them), `List` for every collection (bags have fetch implications), and
  bidirectional associations without a helper that sets both sides.
- `hibernate.ddl-auto=update` only in local development; Flyway or Liquibase everywhere
  else. `spring.jpa.show-sql` never in production; `logging.level.org.hibernate.SQL=debug`
  when needed.
- HikariCP stays small (cores times two, up to about 20) even with virtual threads; set
  `connection-timeout` consciously. The pool is the backpressure point.

## Error responses

- RFC 9457 `ProblemDetail`. `spring.mvc.problemdetails.enabled=true` and one
  `@RestControllerAdvice extends ResponseEntityExceptionHandler` with
  `@ExceptionHandler` methods returning `ProblemDetail`, or domain exceptions extending
  `ErrorResponseException` so the base handler renders them. The content type becomes
  `application/problem+json`, and clients decode it with
  `RestClientResponseException.getResponseBodyAs(ProblemDetail.class)`.
- Extra fields through `ProblemDetail.setProperty(...)`, which serialises top-level.
  Localise `type`, `title` and `detail` through `MessageSource` keys
  `problemDetail.type.<FQCN>` and friends.
- Validation failures (`MethodArgumentNotValidException`,
  `HandlerMethodValidationException`) reach the base handler, which builds the 400 with
  per-field errors. No per-controller catching.
- Order a custom advice ahead of Boot's auto-configured one (`@Order` below 0); do not
  extend both. Boot's `BasicErrorController` for unhandled errors may still emit the
  legacy `timestamp/status/error/path` body in some versions; handle everything you care
  about in the advice.
- No ad hoc `ResponseEntity<Map<String, Object>>` error bodies and no `@ResponseStatus`
  on exception classes in new code.
- Never leak stack traces, SQL or internal hostnames. The real error, with the trace id,
  goes to the log once, in the advice.

## Logging

- SLF4J (`private static final Logger log = LoggerFactory.getLogger(Foo.class)`, or
  Lombok `@Slf4j`), never `java.util.logging`, `System.out`, or Logback and Log4j APIs
  directly. Boot routes everything through SLF4J.
- Parameterised messages with `{}`; never concatenation or `String.format` in the call.
  Formatting is skipped when the level is off, and concatenation is how log injection
  happens. `isDebugEnabled()` guards only when building an argument is expensive.
- The throwable goes last, unformatted: `log.warn("Retry {} failed for {}", n, id, e)`.
  Not `e.getMessage()` alone, not a `{}` for the exception.
- Levels: ERROR means someone must act now (lost data, a dependency failed with no
  fallback); WARN means handled but abnormal (retry succeeded, fallback used); INFO means
  a business lifecycle event (started, order placed, job finished with counts), one or
  two per request; DEBUG is developer diagnostics; TRACE is per-item detail. Never log
  and rethrow at the same level; log once at the boundary that handles the exception.
- Never log passwords, tokens, API keys, session ids, cookies, `Authorization` headers,
  connection strings, card numbers, government ids, health data, or full request bodies.
  Mask or hash when a correlation is needed. A record's `toString` includes every
  component; keep secrets out of records that get logged, or wrap them.
- Structured JSON through Boot 3.4+: `logging.structured.format.console=ecs` (or
  `logstash`, `gelf`); `logging.structured.ecs.service.name` defaults to
  `spring.application.name`; add, drop and rename fields with
  `logging.structured.json.add.*`, `.exclude` and `.rename`. MDC entries become JSON
  fields. No `logstash-logback-encoder` dependency on a Boot 3.4+ project.
- Per-event fields with the SLF4J 2 fluent API:
  `log.atInfo().setMessage("Order placed").addKeyValue("orderId", id).log()`. Always end
  with `.log()`; the builder methods are `@CheckReturnValue` and a dropped `.log()` logs
  nothing.
- MDC for request-scoped context (trace id, tenant, user id): set in a filter, cleared
  in `finally` or through `MDC.putCloseable` in try-with-resources. A leaked value
  attaches to the next request on that thread. Micrometer Tracing already populates
  `traceId` and `spanId`; do not reinvent it. With WebFlux, context propagation rather
  than MDC directly.
- Structured JSON also defeats log forging: CR and LF in user input cannot fake an entry.
  Cap the size of logged user input.
- Log at the edges: the controller advice logs a failure once with context; services do
  not log successful paths at INFO per method.

## Virtual threads in Boot

- `spring.threads.virtual.enabled=true` (Java 21+, Boot 3.2+) moves Tomcat and Jetty
  request handling, `applicationTaskExecutor` (`@Async`, MVC async), `taskScheduler`,
  Kafka and RabbitMQ listener containers, Redis cluster commands and Pulsar containers
  onto virtual threads. One switch; know the blast radius.
- With the flag on, `spring.task.execution.pool.*`, `spring.task.scheduling.pool.size`
  and `server.tomcat.threads.max` are ignored. Tuning them is editing dead config.
  `TaskDecorator` beans and thread-name prefixes still apply.
- The flag is not a performance switch. Benchmark, and on Java 21 check third-party
  libraries (tracing agents, mail, old JDBC drivers) for pinning storms with JFR
  `jdk.VirtualThreadPinned`.
- Size HikariCP for the database, not the thread count; see JPA above.

## JDK management with jolta

[jolta](https://github.com/OneAppPlatform/jolta) is the JDK manager for every project
that uses this standard: like Volta, but for Java. It pins a JDK per project in
`.java-version` and puts shims for `java`, `javac`, `jar`, `jshell` and friends on the
`PATH`. Each shim walks up from the current directory to the nearest `.java-version`,
resolves an installed JDK, sets `JAVA_HOME` and execs the real binary. No `sdk use`, no
`jenv shell`, no `export JAVA_HOME`, and the right JDK whichever directory you are in.

Install once per machine, then verify:

```sh
brew install OneAppPlatform/tap/jolta && jolta setup    # macOS or Linux with Homebrew
# or: curl -fsSL https://raw.githubusercontent.com/OneAppPlatform/jolta/main/install.sh | sh
jolta doctor
```

`setup` installs into `~/.jolta`, generates shims, and adds two marked blocks to the
shell profile: the shims on `PATH`, and a hook that keeps `JAVA_HOME` in sync in
interactive shells. Windows installs from the release zip and runs `jolta.exe setup`.

Daily use:

| Need | Command |
|---|---|
| Pin this project's JDK (writes `.java-version`; commit it) | `jolta pin 21`, `jolta pin corretto@21` |
| See which JDK resolves here | `jolta current`, `jolta which java` |
| Install a JDK | `jolta install 21`, `jolta install graalvm@25` |
| Run a tool with `JAVA_HOME` and `PATH` set for this project | `jolta exec ./gradlew test` |
| Export `JAVA_HOME` in a script or non-interactive shell | `eval "$(jolta env)"` or `export JAVA_HOME="$(jolta home)"` |
| List installed JDKs, see what is available | `jolta list`, `jolta catalog` |
| Point releases | `jolta outdated`, `jolta upgrade`, `jolta prune` |
| Maven toolchains.xml from the installed JDKs | `jolta toolchains --write` |
| Something is off | `jolta doctor` |

Rules that follow from it:

- Every project commits a `.java-version`. A `<spec>` is a major (`21`), an exact
  version (`21.0.4`), or distro-qualified (`temurin@21`, `corretto@21`). Pin the major
  unless the project needs an exact build; the point release then floats with
  `jolta upgrade`. Change the file only to move the project to a new JDK, as its own
  reviewed commit.
- Never install a JDK with brew, sdkman, jenv or an installer package, and never set
  `JAVA_HOME` by hand or in a dotfile. Two sources of truth for the JDK is how "works on
  my machine" happens.
- Run the build wrapper directly (`./gradlew`, `./mvnw`); the shims give it the pinned
  JDK. The shell hook does not run in non-interactive shells, so a script or an agent
  that needs `JAVA_HOME` wraps the command in `jolta exec` or evaluates `jolta env`
  first.
- CI installs jolta, runs `jolta install "$(cat .java-version)"` and then
  `jolta exec ./gradlew build`, so CI and laptops build with the same JDK for the same
  reason. `jolta mirror` builds an offline JDK mirror for air-gapped runners
  (`JOLTA_DOWNLOAD_BASE`).
- Gradle toolchains and `options.release` still describe the bytecode target inside the
  build; jolta decides which JDK runs the build. The two agree when `.java-version` and
  the toolchain's `languageVersion` name the same major.
- When a tool reports the wrong Java version, `jolta doctor` first, then `jolta current`
  in the project directory. The usual causes are a stray `JAVA_HOME` export in a profile
  or a missing `.java-version` falling back to `jolta default`.

## Build and tooling

- Formatter: google-java-format through Spotless (`googleJavaFormat('<pinned>')`,
  `formatAnnotations()`, `removeUnusedImports()`), `spotlessCheck` in CI, `ratchetFrom
  'origin/main'` when adopting on a legacy repo. `formatAnnotations` fixes JSpecify
  type-annotation placement. Two-space indentation is Google style; `.aosp()` for four.
- Error Prone on every compile (`net.ltgt.errorprone`), `-Werror` for ERROR patterns.
  High-value checks: `MissingOverride`, `ReferenceEquality`, `EqualsHashCode`,
  `ReturnValueIgnored`, `FutureReturnValueIgnored`, `MissingCasesInEnumSwitch`,
  `UnusedVariable`, `StringSplitter`, `DefaultCharset`, `JavaTimeDefaultTimeZone`,
  `CatchAndPrintStackTrace`, `DoNotMock`, `ThrowNull`, `ArrayEquals`. `MissingDefault`
  off, since it conflicts with exhaustive sealed switches.
- NullAway as an Error Prone plugin at ERROR, `-XepOpt:NullAway:OnlyNullMarked=true`,
  `CustomContractAnnotations=org.springframework.lang.Contract`, generated code excluded.
  `JSpecifyMode=true` only after the baseline is clean; it needs a JDK 22+ toolchain
  even when targeting 17 or 21 with `--release`.
- Checkstyle with `google_checks.xml` or a trimmed copy for the semantic rules: naming,
  import control, Javadoc on public API, `FinalClass`, `HideUtilityClassConstructor`,
  `IllegalCatch`. `MissingSwitchDefault` disabled. Formatting is the formatter's job.
- SpotBugs with `find-sec-bugs`, `effort = max`, `reportLevel = high` to start; triage
  with `@SuppressFBWarnings(justification = ...)`.
- JaCoCo `jacocoTestCoverageVerification` wired into `check`: a `BUNDLE` rule on line
  and branch `COVEREDRATIO` set at the current value and ratcheted up, with generated
  code, configuration classes and the `Application` class excluded. Thresholds are
  policy; mutation testing (PIT) is the better signal when affordable.
- Gradle: Kotlin DSL, version catalog, `java.toolchain.languageVersion`, `options.release`
  when the bytecode target is lower than the build JDK, `useJUnitPlatform()`. The Boot
  plugin already sets `-parameters`, needed for constructor binding. Maven:
  `spring-boot-starter-parent` handles the same. Run the wrapper (`./gradlew`, `./mvnw`)
  rather than a globally installed build tool; jolta supplies the JDK (see above).
- The Spring Boot BOM governs versions; do not pin Jackson, Hibernate or Mockito by
  hand. `spring-boot-properties-migrator` temporarily on upgrades, then remove it.
  OpenRewrite recipes for mechanical Boot migrations.
- CI order: formatter check, compile with Error Prone and NullAway, unit tests, ArchUnit
  or Modulith verify, integration tests with Testcontainers, JaCoCo verification. Fail on
  the cheapest check first.
