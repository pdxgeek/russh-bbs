# Testing

Tests prove behaviour through the public surface and run without Spring wherever they
can. Sources: the JUnit 5 user guide, AssertJ docs, the Mockito wiki ("How to write good
tests") and Mockito 5 release notes, the Spring Boot reference on testing and
Testcontainers, the Spring Boot 4.0 migration guide, the ArchUnit user guide, Spring
Modulith.

Contents: Version notes · Shape and naming · Assertions · Mocking · The pyramid in Spring ·
Slices · Testcontainers · Context caching · Architecture tests · Fixtures

## Version notes

- Mockito 5 uses the inline mock maker by default: final classes and methods are
  mockable, `mockStatic` is available, and the `mockito-inline` dependency is obsolete.
  `any(String[].class)` matches zero or more varargs; `any(String.class)` matches one.
- Spring Boot 3.4 deprecated `@MockBean` and `@SpyBean`; Boot 4 removed them. Use
  `@MockitoBean` and `@MockitoSpyBean` (Framework 6.2+). `@MockitoBean` is not allowed
  inside a `@Configuration` class; use `@MockitoBean(types = ...)` on the test class.
- Boot 4 `@SpringBootTest` no longer auto-configures `MockMvc` or `TestRestTemplate`.
  Add `@AutoConfigureMockMvc` or `@AutoConfigureTestRestTemplate`, or use
  `RestTestClient`. `MockMvcTester` (Boot 3.4+) is AssertJ-based.
- Testcontainers `@ServiceConnection` (Boot 3.1+) replaces `@DynamicPropertySource` for
  supported images.

## Shape and naming

- JUnit 5. Test classes and methods are package-private, no `public`, no `test` prefix.
  `@DisplayNameGeneration(ReplaceUnderscores.class)` or `@DisplayName` for readable
  reports. `@Nested` classes group by method or scenario.
- One naming pattern per repo, enforced. `methodUnderTest_condition_expectedResult` or
  `should_<expectation>_when_<condition>` in snake_case with `ReplaceUnderscores` both
  work; Google style prefers camelCase. The hard rule is consistency, and the name states
  the expected behaviour.
- Arrange, act, assert, separated by blank lines. One behaviour per test. No logic in
  tests: a `for` loop with assertions stops at the first failure and hides the input.
- `@ParameterizedTest` with `@ValueSource`, `@CsvSource`, `@EnumSource`,
  `@NullAndEmptySource`, `@MethodSource` instead of copy-pasted tests.
- A test that cannot fail is deleted. Coverage is a floor that catches untested files,
  not a target.

## Assertions

- AssertJ everywhere: `assertThat(x).isEqualTo(y)`, `containsExactly`, `extracting`,
  `isInstanceOf`, `assertThatThrownBy(...).isInstanceOf(X.class).hasMessageContaining(...)`.
  No `assertEquals(expected, actual)` with its argument-order mistakes, no Hamcrest in
  new code.
- `assertAll` or AssertJ soft assertions when several independent facts matter in one
  test.
- Assert the exact statement that throws, with `assertThatThrownBy`, never try/fail.

## Mocking

- Do not mock types you do not own: JDK collections, `ObjectMapper`/`JsonMapper`,
  `RestClient`, JPA `EntityManager`. Wrap them behind your own interface or test with the
  real thing. Third-party behaviour changes keep passing mocked tests. Error Prone
  `DoNotMock` can enforce it.
- Do not mock value objects or records; build them. A test where every collaborator is a
  mock tests the mocks.
- `@ExtendWith(MockitoExtension.class)` with its default strict stubs: an unused
  `when(...)` fails with `UnnecessaryStubbingException`. Delete the stub rather than
  mark it `lenient()`; an unused stub means the test is wrong.
- Call the constructor explicitly rather than `@InjectMocks`. `new Service(mockA,
  mockB)` is compile-checked; `@InjectMocks` leaves fields null when the constructor
  changes.
- Verify interactions only for side effects (a message sent, an event published). Assert
  state and return values otherwise. Never `verifyNoMoreInteractions` by habit;
  over-specified tests break on refactors.

## The pyramid in Spring

| Layer | Test style | Spring? |
|---|---|---|
| Domain rules, services | Plain JUnit, constructor-built, fakes or mocks for ports | No |
| Controllers | `@WebMvcTest(Controller.class)` with `@MockitoBean` services | Slice |
| Repositories and queries | `@DataJpaTest` with Testcontainers and `Replace.NONE` | Slice |
| End to end | A few `@SpringBootTest(webEnvironment = RANDOM_PORT)` | Full |

Slices load a restricted context and run in milliseconds. One slice annotation per test
class; combining slices is unsupported.

## Slices

- `@WebMvcTest` scans only web components: `@Controller`, `@ControllerAdvice`,
  `Filter`, converters, `WebMvcConfigurer`. `@Service` and `@ConfigurationProperties`
  beans are absent, so mock them or `@Import` what the test needs;
  `@EnableConfigurationProperties` for properties records. "No qualifying bean" is the
  usual symptom of forgetting this.
- `@DataJpaTest` is transactional with rollback after each test, swaps the `DataSource`
  for an embedded database when one is on the classpath, and turns on `show-sql`. For a
  real database add `@AutoConfigureTestDatabase(replace = Replace.NONE)` and a
  Testcontainers `@ServiceConnection` container. H2 is not Postgres; `Replace.NONE` is
  the line generated tests forget.
- Do not wrap integration tests in `@Transactional` for cleanup. It keeps the session
  open so lazy loading works in the test and fails in production. Clean with `@Sql` or a
  truncate step. Repository-only `@DataJpaTest` tests are the acceptable exception.

## Testcontainers

- `@Testcontainers` on the class, a `static @Container` field, `@ServiceConnection` on
  it. No `@DynamicPropertySource` for supported images.
- Share containers across classes by declaring them in a `@TestConfiguration` imported
  with `@Import`, or in an interface with `@ImportTestcontainers`. Static fields outlive
  Spring's cached contexts; instance fields do not, and a per-method container with a
  cached context yields "connection refused" in the next class.
- Assert on the result and on the table state. A repository function that returns the
  right value and writes the wrong row passes a result-only test.

## Context caching

Keep test configuration identical across classes: the same properties, the same set of
`@MockitoBean`s, the same `@Import`s. Every unique combination is a new context costing
five to ten seconds, which is the main reason slow suites are slow.

## Architecture tests

One `@AnalyzeClasses` test holding the rules that otherwise rot:

- layer direction, `noClasses().that().resideInAPackage("..domain..").should()
  .dependOnClassesThat().resideInAnyPackage("..web..", "..persistence..")`;
- no cycles, `slices().matching("com.acme.(*)..").should().beFreeOfCycles()`;
- naming and annotation rules: controllers live in `..web..`, `@Transactional` never on
  a controller.

With Spring Modulith, `ApplicationModules.of(App.class).verify()` covers module
boundaries and cycles.

## Fixtures

Builders or factory methods returning a full valid object with overridable fields
(`anOrder().withStatus(CANCELLED).build()`), or plain constructors, or Instancio. Never
mocks of data. An object that is hard to build in a test is telling you the production
design needs work.
