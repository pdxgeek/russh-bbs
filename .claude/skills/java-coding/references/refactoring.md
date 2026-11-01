# Refactoring existing code (sub-skill)

Changing code that already works is a different job from writing new code. The standard
in SKILL.md still applies to the result; this file is about getting there without
changing what the code does. Read it for cleanups, modernisations, "make this a stream",
and reviews of someone else's diff.

Contents: Method · The moves, by smell · Worked examples · Reporting

## Method

1. **Equivalence first.** Before converting anything, name what the old code does that
   the new code must still do: evaluation order, side effects, distinct log lines, which
   exceptions escape, eager versus lazy defaults, whether callers mutate a returned
   list. A conversion that quietly merges two different empty-branches, broadens a
   catch, or returns an unmodifiable list to a caller that adds to it is a bug wearing a
   cleanup's clothes. When a conversion does change something (a lookup now runs before
   a role check), either verify the change is harmless and say so in the report, or keep
   the guard.
2. **Delete before you convert.** Check compound conditions for arms subsumed by another
   arm and lookups duplicated by a central checker. Dead branches and null checks on
   values that are non-null by contract come out first. The best refactor of redundant
   code is removal.
3. **One move per smell.** Apply the move from the table, compile, run the tests, then
   look for the next smell. Several moves in one step hide which one broke the test.
4. **Chains are the default, not a religion.** Prefer the Optional or stream chain when
   it is behaviourally identical and at least as readable. Keep the imperative form when
   the body mutates locals, needs break or early exit across iterations, or a builder
   API is inherently imperative. Never force a chain that needs contortions; fix the
   reason for the contortion.
5. **Named helpers make chains readable.** A pipeline reads as a sentence only when each
   stage is a short expression or a well-named call. Extract fat lambdas into private
   methods whose names state intent (`requireStripeCustomer`, `resolvePayeeEmail`).
   This is also the standard unblocking move: a callee that throws a checked exception
   cannot sit in a lambda, so a helper translates the failure into the domain's runtime
   exception, then the chain proceeds.
6. **Push work to the source.** An existence test over repository results is an
   `existsBy...` query. Repository list results are never null, so null-tolerant
   wrappers on them are dead code.
7. **Verify with the project's tests.** Compile with the project's checks and run the
   tests scoped to the touched code before claiming the refactor is safe. Add a test
   first when the behaviour being preserved has none.

## The moves, by smell

| Smell in the code | Move |
|---|---|
| `findOne(...).orElse(null)` then null check | `map`/`filter`/`flatMap` chain (example 1) |
| Null guard on a value the next line wraps anyway | Start from `Optional.ofNullable(x)` (examples 2, 5) |
| Two early returns, one of them logs | `.or(() -> { log...; return Optional.empty(); })` or keep the guard; never merge distinct empties silently (example 3) |
| try/catch blocking a chain | Extract a translating helper (example 4) |
| `isPresent()` + `get()` | The chaining API; `orElseThrow()` when presence is proven |
| `!isEmpty(repo.findAll...)` | Derived `existsBy...` query (example 6) |
| Hand-rolled pair, parallel locals, `Map.Entry` abuse | Private or local record (example 7) |
| sort then `get(0)` | `min`/`max` with a composed `Comparator`, unique-key tiebreak (example 8) |
| `filter(...).count() > 0` | `anyMatch` |
| `forEach` accumulating into a collection | A collector (`groupingBy`, `toMap`, `toList()`) |
| Unreadable one-line mega-chain | `var` intermediates with role-revealing names, or a local record |
| `instanceof` chain over a sealed type, or a type-code `switch` with `default` | Pattern `switch` with record deconstruction, no `default` (example 10) |
| Cast after an `instanceof` check | Enhanced `instanceof` binding, negated-guard idiom |
| Switch statement with `break`s and a result variable | Switch expression with `->` |
| Class with final fields, all-args constructor, getters, `equals`/`hashCode` | A record, with validation in the compact constructor |
| String concatenation for SQL, JSON or HTML literals | Text block with `.formatted(...)` |
| `Collections.unmodifiableList(new ArrayList<>(x))` | `List.copyOf(x)` |
| `Arrays.asList(...)` or `new HashSet<>(Arrays.asList(...))` | `List.of`, `Set.of`, after checking for nulls and duplicates |
| `new Thread`, fixed thread pools for I/O fan-out | A virtual-thread executor in try-with-resources, bounded with a `Semaphore` |
| `synchronized` rewritten to `ReentrantLock` "for virtual threads" on Java 24+ | Leave it; `synchronized` no longer pins |
| Field `@Autowired` | Constructor injection, `private final` fields |
| `@Transactional` on a private or self-invoked method | Move the method to another bean |
| `catch (Exception e) { log.error(...); }` inside a transactional method | Rethrow a domain exception, or `rollbackFor`; the catch commits partial work |
| `log.info("x=" + x)`; throwable not last; `e.printStackTrace()` | `{}` placeholders, throwable last |
| `@MockBean`, `mockito-inline`, `@InjectMocks` | `@MockitoBean`, drop the dependency, call the constructor |
| Lombok `@Data` on an entity | `@Getter`/`@Setter`, hand-written id-based `equals`/`hashCode`, `toString` without associations |
| `GenerationType.IDENTITY` on Postgres | `SEQUENCE` with `allocationSize`, batch settings |
| `@Nullable public Foo bar()` | `public @Nullable Foo bar()` (type-use position) |
| `Date`, `Calendar`, `SimpleDateFormat`, `new Date()` for now | `java.time` with an injected `Clock` |
| `StringBuilder` for a single-expression concatenation | Plain `+`; the compiler does better |
| `export JAVA_HOME=...` in a script, README or CI step; sdkman or jenv instructions | `.java-version` plus `jolta exec` / `jolta env` |

## Worked examples

Real refactors, genericised. Each shows the shape to hunt for and the shape to produce,
with the reasoning that decided it. These all preserved behaviour exactly, and that was
verified before committing, not assumed.

### 1. Guard plus orElse(null) into a single chain

Hunt for: `findOne(...).orElse(null)`, a null check, an early return, then work.

```java
// Before
EntityDTO dto = service.findOne(id).orElse(null);
if (dto == null) {
    return null;
}
if (!checker.matches(id, secret)) {
    return null;
}
return dto.getUserId();

// After
return service.findOne(id)
        .filter(entity -> checker.matches(id, secret))
        .map(EntityDTO::getUserId)
        .orElse(null);
```

A `filter` predicate that ignores the element is fine when it reads as an authorization
or precondition gate. The chain communicates "value flows through checks" better than
three exits. The trailing `orElse(null)` stays because the caller is a nullable-consuming
legacy API; inside your own code the Optional would be returned.

### 2. Start the chain from the nullable input

Hunt for: a null guard on a parameter followed by a chain that could have absorbed it.

```java
// Before
if (accountHolderId == null) {
    return Optional.empty();
}
return Optional.ofNullable(api.createSession(accountHolderId, flag))
        .map(session -> new SessionDTO(accountHolderId, session.getClientSecret()));

// After
return Optional.ofNullable(accountHolderId)
        .map(holderId -> api.createSession(holderId, flag))
        .map(session -> new SessionDTO(accountHolderId, session.getClientSecret()));
```

`map` already turns a null mapper result into empty, so the intermediate call that "might
return null" needs no extra guard.

### 3. Keep a branch-specific log with or()

Hunt for: two early returns that would collapse into one chain except one of them logs.

```java
// Before
if (property == null) {
    return Optional.empty();
}
PayoutSettingDTO setting = property.getPayoutSetting();
if (setting == null) {
    log.warn("Payout setting missing for property {}", property.getId());
    return Optional.empty();
}
// ... proceed with setting

// After
return Optional.ofNullable(property)
        .flatMap(prop -> Optional.ofNullable(prop.getPayoutSetting()).or(() -> {
            log.warn("Payout setting missing for property {}", prop.getId());
            return Optional.empty();
        }))
        .flatMap(setting -> { /* proceed */ });
```

The two kinds of empty behaved differently (one warned), and that difference is part of
the behaviour. Do not merge distinct empties silently; carry the distinction in `.or()`
or keep the guard.

### 4. Extract a helper to absorb try/catch (checked exceptions)

Hunt for: a chain conversion blocked because a callee throws a checked exception, which
cannot escape a lambda.

```java
// Before: forced out of a chain by the checked exception
EntityDTO dto = service.findOne(id).orElse(null);
if (dto == null) {
    return Optional.empty();
}
Customer customer;
try {
    customer = api.getCustomerOrCreate(dto, user);   // throws StripeException
} catch (StripeException e) {
    throw new BadRequestAlertException(e.getUserMessage(), ENTITY, "errorKey");
}
return buildSession(customer.getId());

// After
return service.findOne(id)
        .flatMap(dto -> buildSession(requireCustomer(dto).getId()));

private Customer requireCustomer(EntityDTO dto) {
    try {
        return api.getCustomerOrCreate(dto, resolveUser(dto));
    } catch (StripeException e) {
        throw new BadRequestAlertException(e.getUserMessage(), ENTITY, "errorKey");
    } catch (Exception e) {
        // The interface declares throws Exception, but the implementation only throws StripeException.
        throw new IllegalStateException(e);
    }
}
```

The helper translates the checked failure into the domain's runtime exception once, and
every caller gets to stay inline. Before writing the broad catch, read the callee: if its
`throws Exception` is wider than what the body can actually throw, say so in a comment
and treat the impossible arm as a defect (`IllegalStateException`), never as a swallowed
error. Do not silently broaden what a catch converts; that changes behaviour.

### 5. Turn a pre-check into a filter

Hunt for: an if that returns empty before a terminal ofNullable.

```java
// Before
if (SecurityUtils.isCurrentUserInRole(ADMIN_AUTHORITIES)) {
    return Optional.empty();
}
return Optional.ofNullable(userService.getCurrentUser());

// After
return Optional.ofNullable(userService.getCurrentUser())
        .filter(caller -> !SecurityUtils.isCurrentUserInRole(ADMIN_AUTHORITIES));
```

Note the evaluation order flips: the lookup now runs before the role check. That was fine
here because the lookup is a cheap in-memory read. When the reordered call is expensive or
has side effects, keep the guard.

### 6. Existence checks belong in the query, not the stream

Hunt for: `!CollectionUtils.isEmpty(repository.findAllBy...(...))`,
`findAllBy...(...).size() > 0`, or `.stream().anyMatch(...)` over a full fetch.

```java
// Before
boolean granted = !CollectionUtils.isEmpty(
        repository.findAllBySecretKeyAndApplicationIdAndActiveIsTrue(secret, appId));

// After: derived exists query on the repository
boolean granted = repository.existsBySecretKeyAndApplicationIdAndActiveIsTrue(secret, appId);
```

Spring Data list queries never return null, so the null half of `CollectionUtils.isEmpty`
was dead; and materializing rows to test existence pays for a fetch the database could
answer with EXISTS. The lesson generalizes: push filtering and existence into the source
when the source is a database.

### 7. A private record for an intermediate result

Hunt for: parallel local variables, or a chain that needs to carry two values.

```java
public enum Source { APPLICATION, APPLICATION_PAYMENT, DEFAULT_CLAIM }

private record Resolution(Source source, Long applicationId) { }

private Optional<Resolution> resolve(String secretKey) {
    return applicationRepository.findBySecret(secretKey)
            .map(app -> new Resolution(Source.APPLICATION, app.getId()))
            .or(() -> paymentRepository.findBySecret(secretKey)
                    .map(payment -> new Resolution(Source.APPLICATION_PAYMENT, payment.getApplicationId())))
            .or(() -> claimRepository.findBySecret(secretKey)
                    .map(claim -> new Resolution(Source.DEFAULT_CLAIM, claim.getApplicationId())));
}
```

The record names the pair, costs three lines, and lets the chain return one value instead
of contorting around two. Keep it private and nested when only one class cares.

### 8. min with a Comparator instead of sort-and-take-first

```java
// The deterministic "pick one" from a collection
return userRepository.findAllByPropertyId(propertyId).stream()
        .min(Comparator
                .comparing((User user) -> StringUtils.defaultString(user.getFirstName()), String.CASE_INSENSITIVE_ORDER)
                .thenComparing(user -> StringUtils.defaultString(user.getLastName()), String.CASE_INSENSITIVE_ORDER)
                .thenComparing(User::getId))
        .or(this::fallbackUser)
        .map(user -> StringUtils.defaultIfBlank(user.getEmail(), user.getLogin()))
        .orElse(null);
```

`min` with a composed comparator replaces sort-then-get(0), never throws on empty, and the
tie-breaking `thenComparing(User::getId)` is what makes the pick stable across runs. The
`orElse(null)` at the end is a boundary concession: the consumer is a legacy API that takes
a nullable value. Inside your own code, keep the Optional.

### 9. Delete redundant arms instead of chaining them

Not every cleanup is a conversion. One authorization check ORed a local repository lookup
with a call to the central checker; the local lookup queried a strict subset of what the
checker already covered, so `local || checker` reduced to `checker` in every legitimate
state. The fix was deletion, and the chain got simpler for free. Before converting a
compound condition, ask whether one side subsumes the other.

### 10. A sealed result instead of an instanceof chain

Hunt for: an `instanceof` chain or a type-code `switch` over a closed set of outcomes,
with a `default` that throws or returns a placeholder.

```java
// Before
Object outcome = payments.charge(order);
if (outcome instanceof Receipt) {
    Receipt receipt = (Receipt) outcome;
    return confirm(order, receipt.reference());
} else if (outcome instanceof Declined) {
    return reject(order, ((Declined) outcome).reason());
} else {
    throw new IllegalStateException("unknown outcome " + outcome);
}

// After
sealed interface ChargeOutcome permits Receipt, Declined, Pending {}
record Receipt(String reference) implements ChargeOutcome {}
record Declined(String reason) implements ChargeOutcome {}
record Pending(Instant retryAfter) implements ChargeOutcome {}

return switch (payments.charge(order)) {
    case Receipt(var reference) -> confirm(order, reference);
    case Declined(var reason) -> reject(order, reason);
    case Pending(var retryAfter) -> schedule(order, retryAfter);
};
```

The `else` branch existed because the type system could not say the set was closed. With
the sealed interface it can, so the `default` disappears and the compiler reports every
switch that forgets `Pending`. Adding `Pending` to the old code would have compiled
cleanly and thrown at runtime. When the selector can be null, add `case null`; a pattern
switch throws on null otherwise.

## Reporting

State what changed and what was proven equal, in plain sentences. When a conversion was
declined, give the specific blocker (checked exception, mutation ordering, distinct
logging, a caller that mutates the list), not a vague preference. When behaviour differs
in any observable way, that is the headline of the report, not a footnote. Name the tests
that cover the change.
