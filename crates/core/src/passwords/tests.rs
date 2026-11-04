#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::too_many_lines,
    clippy::unused_async_trait_impl,
    clippy::assert_is_empty
)]

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use argon2::password_hash::PasswordHasher;

use super::hash::{self, DUMMY_HASH};
use super::*;
use crate::access::{Permission, Slug, Snapshot, Via};
use crate::audit::{Action, AuditEvent};
use crate::sanitize::sanitize_username;
use crate::store::{Account, AccountId, AccountState, StoreError, UnixSeconds};

const GOOD: &str = "correct horse battery";
const OTHER: &str = "another long passphrase";

fn id(n: i64) -> AccountId {
    AccountId::from_raw(n)
}

fn account(n: i64, handle: &str, state: AccountState) -> Account {
    Account {
        id: id(n),
        username: sanitize_username(handle).unwrap(),
        state,
        created_at: UnixSeconds::from_raw(0),
    }
}

fn holds(n: i64, permissions: &[Permission]) -> Snapshot {
    Snapshot::from_role(
        Some(id(n)),
        &Slug::MEMBER,
        10,
        &Via::Account,
        permissions.iter().copied(),
    )
}

fn member(n: i64) -> Snapshot {
    holds(n, &[Permission::AccountSetPassword])
}

/// A hash made with weaker parameters, as an older server would have stored it.
fn weak_hash(password: &str) -> String {
    let params = argon2::Params::new(8 * 1024, 1, 1, None).unwrap();
    argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params)
        .hash_password_with_salt(password.as_bytes(), b"0123456789abcdef")
        .unwrap()
        .to_string()
}

#[derive(Default)]
struct Fake {
    rows: Mutex<Vec<(Account, Option<String>)>>,
    audits: Mutex<Vec<AuditEvent>>,
    /// Whether the accounts have no SSH key (a password account, D94).
    keyless: Mutex<bool>,
}

impl Fake {
    fn with(rows: Vec<(Account, Option<String>)>) -> Self {
        Self {
            rows: Mutex::new(rows),
            audits: Mutex::default(),
            keyless: Mutex::new(false),
        }
    }

    fn hash_of(&self, n: i64) -> Option<String> {
        self.rows
            .lock()
            .unwrap()
            .iter()
            .find(|(a, _)| a.id == id(n))
            .and_then(|(_, h)| h.clone())
    }
}

impl PasswordStore for Fake {
    async fn login_record(&self, handle: &str) -> Result<Option<LoginRecord>, StoreError> {
        Ok(self
            .rows
            .lock()
            .unwrap()
            .iter()
            .find(|(a, _)| a.username.as_str().to_ascii_lowercase() == handle)
            .map(|(account, hash)| LoginRecord {
                account: account.clone(),
                hash: hash.clone(),
            }))
    }

    async fn stored_hash(&self, account: AccountId) -> Result<Option<String>, StoreError> {
        self.rows
            .lock()
            .unwrap()
            .iter()
            .find(|(a, _)| a.id == account)
            .map(|(_, h)| h.clone())
            .ok_or(StoreError::AccountNotFound)
    }

    async fn link_key(
        &self,
        _account: AccountId,
        _fingerprint: &crate::access::Fingerprint,
        _audit: &AuditEvent,
    ) -> Result<(), StoreError> {
        Ok(())
    }

    async fn has_key(&self, _account: AccountId) -> Result<bool, StoreError> {
        Ok(!*self.keyless.lock().unwrap())
    }

    async fn set_hash(
        &self,
        account: AccountId,
        hash: Option<&str>,
        audit: &AuditEvent,
    ) -> Result<(), StoreError> {
        let mut rows = self.rows.lock().unwrap();
        let row = rows
            .iter_mut()
            .find(|(a, _)| a.id == account)
            .ok_or(StoreError::AccountNotFound)?;
        let had = row.1.is_some();
        row.1 = hash.map(str::to_owned);
        if hash.is_some() || had {
            self.audits.lock().unwrap().push(audit.clone());
        }
        Ok(())
    }

    async fn upgrade_hash(
        &self,
        account: AccountId,
        old: &str,
        new: &str,
    ) -> Result<bool, StoreError> {
        let mut rows = self.rows.lock().unwrap();
        let row = rows.iter_mut().find(|(a, _)| a.id == account).unwrap();
        if row.1.as_deref() == Some(old) {
            row.1 = Some(new.to_owned());
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

/// Counts how many slow jobs ran.
#[derive(Default)]
struct Counting(AtomicUsize);

impl Offload for Counting {
    fn run<T, F>(&self, work: F) -> impl std::future::Future<Output = Result<T, Unavailable>> + Send
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
    {
        self.0.fetch_add(1, Ordering::SeqCst);
        std::future::ready(Ok(work()))
    }
}

fn secret(text: &str) -> Secret {
    Secret::new(text.to_owned())
}

// ---- hashing ----

#[test]
fn a_hash_verifies_only_its_password_and_carries_its_parameters() {
    let stored = hash::hash(GOOD).unwrap();
    assert!(stored.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"));
    assert!(hash::verify(&stored, GOOD));
    assert!(!hash::verify(&stored, OTHER));
    assert!(!hash::verify(&stored, ""));
    // A fresh salt every time.
    assert_ne!(stored, hash::hash(GOOD).unwrap());
    assert!(!stored.contains(GOOD));
}

#[test]
fn the_dummy_hash_is_current_and_matches_nothing_typed() {
    assert!(!hash::needs_rehash(DUMMY_HASH), "regenerate DUMMY_HASH");
    assert!(argon2::password_hash::phc::PasswordHash::new(DUMMY_HASH).is_ok());
    for guess in ["", "password", GOOD, "dummy", "no-one-has-this"] {
        assert!(!hash::verify(DUMMY_HASH, guess));
    }
}

#[test]
fn an_older_hash_still_verifies_and_is_marked_for_upgrade() {
    let old = weak_hash(GOOD);
    assert!(hash::verify(&old, GOOD));
    assert!(hash::needs_rehash(&old));
    assert!(!hash::needs_rehash(&hash::hash(GOOD).unwrap()));
}

#[test]
fn only_argon2id_within_a_sane_cost_is_verified() {
    let argon2i = "$argon2i$v=19$m=19456,t=2,p=1$ZHVtbXktc2FsdC0xMjM0NQ$xwvTi6nRH+Z7e2rjusTWEqh+rgUtQVcjVqf3FoRaEZk";
    assert!(!hash::verify(argon2i, GOOD));
    let huge = "$argon2id$v=19$m=4000000,t=2,p=1$ZHVtbXktc2FsdC0xMjM0NQ$xwvTi6nRH+Z7e2rjusTWEqh+rgUtQVcjVqf3FoRaEZk";
    assert!(!hash::verify(huge, GOOD));
    for junk in ["", "plain", "$2b$12$abc", "$argon2id$"] {
        assert!(!hash::verify(junk, GOOD));
        assert!(!hash::needs_rehash(junk));
    }
}

// ---- policy ----

#[test]
fn new_passwords_are_length_limited_and_free_of_control_characters() {
    assert_eq!(check_new("short"), Err(PolicyError::TooShort));
    assert_eq!(
        check_new(&"x".repeat(MIN_CHARS - 1)),
        Err(PolicyError::TooShort)
    );
    assert_eq!(check_new(&"x".repeat(MIN_CHARS)), Ok(()));
    assert_eq!(check_new(&"x".repeat(MAX_CHARS)), Ok(()));
    assert_eq!(
        check_new(&"x".repeat(MAX_CHARS + 1)),
        Err(PolicyError::TooLong)
    );
    assert_eq!(
        check_new("twelve chars\u{1b}[31m"),
        Err(PolicyError::ControlCharacter)
    );
    // Length is characters, not bytes.
    assert_eq!(check_new(&"é".repeat(MIN_CHARS)), Ok(()));
    // A passphrase with spaces at the ends is kept as typed.
    assert_eq!(check_new("  spaces around  "), Ok(()));
}

/// Compile-time proof that the types holding typed passwords wipe them when dropped (D81).
#[test]
fn typed_text_is_wiped_on_drop() {
    fn wipes<T: zeroize::ZeroizeOnDrop>() {}
    wipes::<Secret>();
    wipes::<zeroize::Zeroizing<String>>();
    // The screen's input buffer and the copies made for hashing jobs are `Zeroizing<String>`;
    // a plain `String` would not satisfy the bound above and would not compile here.
    let mut screen = PasswordScreen::open(LoginMethod::Key);
    let _ = screen.take_request();
    let _ = screen.deliver(Response::Status(false));
    screen.press(PasswordKey::Char('s'));
    type_text(&mut screen, GOOD);
    // Abandoning the prompt drops the buffer.
    screen.press(PasswordKey::Escape);
    assert!(screen.view().fields.is_empty());
}

#[test]
fn a_secret_never_prints() {
    let text = "hunter2 hunter2 hunter2";
    let request = Request {
        method: LoginMethod::Key,
        kind: Kind::Set {
            current: Some(secret(text)),
            new: secret(text),
        },
    };
    assert!(!format!("{request:?}").contains("hunter2"));
    assert!(!format!("{:?}", secret(text)).contains("hunter2"));
}

// ---- login ----

#[tokio::test]
async fn the_right_password_logs_an_active_account_in() {
    let store = Fake::with(vec![(
        account(1, "Alice", AccountState::Active),
        Some(hash::hash(GOOD).unwrap()),
    )]);
    // Case-insensitive handle, as handles are unique ignoring case.
    for user in ["alice", "ALICE", "  Alice "] {
        match authenticate(&store, &Inline, user, GOOD).await.unwrap() {
            Login::Accepted(found) => assert_eq!(found.id, id(1)),
            Login::Denied => panic!("{user} should log in"),
        }
    }
}

#[tokio::test]
async fn every_failure_is_the_same_denial_and_costs_one_verification() {
    let store = Fake::with(vec![
        (
            account(1, "alice", AccountState::Active),
            Some(hash::hash(GOOD).unwrap()),
        ),
        (account(2, "nopass", AccountState::Active), None),
        (
            account(3, "locked", AccountState::Suspended),
            Some(hash::hash(GOOD).unwrap()),
        ),
        (
            account(4, "newbie", AccountState::Pending),
            Some(hash::hash(GOOD).unwrap()),
        ),
    ]);
    let cases = [
        ("alice", OTHER),
        ("alice", ""),
        ("nobody", GOOD),
        ("nopass", GOOD),
        // Suspended and not-yet-active accounts cannot log in even with the right password.
        ("locked", GOOD),
        ("newbie", GOOD),
        ("", GOOD),
        ("a\u{0}b", GOOD),
        ("x", &"y".repeat(100_000)),
    ];
    for (user, password) in cases {
        let offload = Counting::default();
        let login = authenticate(&store, &offload, user, password)
            .await
            .unwrap();
        assert!(matches!(login, Login::Denied), "{user:?} must be denied");
        assert_eq!(
            offload.0.load(Ordering::SeqCst),
            1,
            "{user:?} must cost exactly one verification, like a wrong password"
        );
    }
}

#[tokio::test]
async fn a_login_upgrades_an_older_hash_once_and_never_overwrites_a_newer_one() {
    let old = weak_hash(GOOD);
    let store = Fake::with(vec![(
        account(1, "alice", AccountState::Active),
        Some(old.clone()),
    )]);
    assert!(matches!(
        authenticate(&store, &Inline, "alice", GOOD).await.unwrap(),
        Login::Accepted(_)
    ));
    let upgraded = store.hash_of(1).unwrap();
    assert_ne!(upgraded, old);
    assert!(!hash::needs_rehash(&upgraded));
    assert!(hash::verify(&upgraded, GOOD));
    // A wrong password does not upgrade anything.
    let store = Fake::with(vec![(
        account(1, "alice", AccountState::Active),
        Some(old.clone()),
    )]);
    let _ = authenticate(&store, &Inline, "alice", OTHER).await.unwrap();
    assert_eq!(store.hash_of(1).unwrap(), old);
    // The compare-and-set: a changed password is not undone by a slower upgrade.
    assert!(
        !store
            .upgrade_hash(id(1), "something else", "x")
            .await
            .unwrap()
    );
    assert_eq!(store.hash_of(1).unwrap(), old);
}

// ---- set and clear ----

#[tokio::test]
async fn a_key_session_sets_and_clears_without_the_current_password() {
    let store = Fake::with(vec![(account(1, "alice", AccountState::Active), None)]);
    set_password(
        &store,
        &Inline,
        &member(1),
        LoginMethod::Key,
        None,
        &secret(GOOD),
    )
    .await
    .unwrap();
    let stored = store.hash_of(1).unwrap();
    assert!(hash::verify(&stored, GOOD));
    assert!(!stored.contains(GOOD));
    assert!(has_password(&store, &member(1)).await.unwrap());
    clear_password(&store, &Inline, &member(1), LoginMethod::Key, None)
        .await
        .unwrap();
    assert!(store.hash_of(1).is_none());
    assert!(!has_password(&store, &member(1)).await.unwrap());
}

#[tokio::test]
async fn a_password_session_must_give_the_current_password() {
    let store = Fake::with(vec![(
        account(1, "alice", AccountState::Active),
        Some(hash::hash(GOOD).unwrap()),
    )]);
    let before = store.hash_of(1).unwrap();
    let new = secret(OTHER);
    for current in [None, Some(secret(OTHER)), Some(secret(""))] {
        let result = set_password(
            &store,
            &Inline,
            &member(1),
            LoginMethod::Password,
            current.as_ref(),
            &new,
        )
        .await;
        assert!(matches!(
            result,
            Err(PasswordError::NeedsCurrent | PasswordError::WrongCurrent)
        ));
        let result = clear_password(
            &store,
            &Inline,
            &member(1),
            LoginMethod::Password,
            current.as_ref(),
        )
        .await;
        assert!(matches!(
            result,
            Err(PasswordError::NeedsCurrent | PasswordError::WrongCurrent)
        ));
    }
    assert_eq!(store.hash_of(1).unwrap(), before, "nothing changed");
    assert!(store.audits.lock().unwrap().is_empty());
    set_password(
        &store,
        &Inline,
        &member(1),
        LoginMethod::Password,
        Some(&secret(GOOD)),
        &new,
    )
    .await
    .unwrap();
    assert!(hash::verify(&store.hash_of(1).unwrap(), OTHER));
}

#[tokio::test]
async fn the_permission_and_the_rules_are_checked_in_core() {
    let store = Fake::with(vec![(account(1, "alice", AccountState::Active), None)]);
    // No `account.set_password`: nothing is read or written.
    let none = holds(1, &[Permission::AccountEditSettings]);
    assert!(matches!(
        set_password(
            &store,
            &Inline,
            &none,
            LoginMethod::Key,
            None,
            &secret(GOOD)
        )
        .await,
        Err(PasswordError::Denied(_))
    ));
    assert!(matches!(
        clear_password(&store, &Inline, &none, LoginMethod::Key, None).await,
        Err(PasswordError::Denied(_))
    ));
    assert!(matches!(
        has_password(&store, &none).await,
        Err(PasswordError::Denied(_))
    ));
    // The waiting room's snapshot has no account and no such permission.
    let waiting = Snapshot::from_role(
        None,
        &Slug::WAITING,
        0,
        &Via::Waiting,
        [Permission::WaitingView],
    );
    assert!(matches!(
        set_password(
            &store,
            &Inline,
            &waiting,
            LoginMethod::Key,
            None,
            &secret(GOOD)
        )
        .await,
        Err(PasswordError::Denied(_))
    ));
    // Policy.
    assert!(matches!(
        set_password(
            &store,
            &Inline,
            &member(1),
            LoginMethod::Key,
            None,
            &secret("short")
        )
        .await,
        Err(PasswordError::Policy(PolicyError::TooShort))
    ));
    assert!(store.hash_of(1).is_none());
}

#[tokio::test]
async fn audit_rows_say_what_happened_and_hold_no_secret() {
    let store = Fake::with(vec![(account(1, "alice", AccountState::Active), None)]);
    // Clearing a password that is not there writes no row.
    clear_password(&store, &Inline, &member(1), LoginMethod::Key, None)
        .await
        .unwrap();
    assert!(store.audits.lock().unwrap().is_empty());
    set_password(
        &store,
        &Inline,
        &member(1),
        LoginMethod::Key,
        None,
        &secret(GOOD),
    )
    .await
    .unwrap();
    clear_password(&store, &Inline, &member(1), LoginMethod::Key, None)
        .await
        .unwrap();
    let audits = store.audits.lock().unwrap();
    let actions: Vec<_> = audits.iter().map(|event| event.action).collect();
    assert_eq!(actions, [Action::PasswordSet, Action::PasswordCleared]);
    for event in audits.iter() {
        let text = format!("{event:?}");
        assert!(!text.contains(GOOD));
        assert!(!text.contains("argon2"));
    }
    assert_eq!(Action::PasswordSet.name(), "password.set");
    assert_eq!(Action::PasswordCleared.name(), "password.cleared");
}

#[tokio::test]
async fn execute_maps_every_outcome_to_an_answer() {
    let store = Fake::with(vec![(account(1, "alice", AccountState::Active), None)]);
    let run = |kind, method, access: Snapshot| {
        let store = &store;
        async move { execute(store, &Inline, &access, Request { method, kind }).await }
    };
    assert_eq!(
        run(Kind::Status, LoginMethod::Key, member(1)).await,
        Response::Status(false)
    );
    assert_eq!(
        run(Kind::Status, LoginMethod::Key, holds(1, &[])).await,
        Response::Refused(Refusal::Denied)
    );
    assert_eq!(
        run(
            Kind::Set {
                current: None,
                new: secret("short")
            },
            LoginMethod::Key,
            member(1)
        )
        .await,
        Response::Refused(Refusal::Policy(PolicyError::TooShort))
    );
    assert_eq!(
        run(
            Kind::Set {
                current: None,
                new: secret(GOOD)
            },
            LoginMethod::Key,
            member(1)
        )
        .await,
        Response::Done(Done::Set)
    );
    assert_eq!(
        run(Kind::Status, LoginMethod::Key, member(1)).await,
        Response::Status(true)
    );
    assert_eq!(
        run(
            Kind::Clear { current: None },
            LoginMethod::Password,
            member(1)
        )
        .await,
        Response::Refused(Refusal::WrongCurrent)
    );
    // An account that does not exist is a failure, not a panic.
    assert_eq!(
        run(Kind::Status, LoginMethod::Key, member(99)).await,
        Response::Refused(Refusal::Failed)
    );
}

// ---- the screen ----

fn type_text(screen: &mut PasswordScreen, text: &str) {
    for c in text.chars() {
        screen.press(PasswordKey::Char(c));
    }
}

fn opened(method: LoginMethod, set: bool) -> PasswordScreen {
    let mut screen = PasswordScreen::open(method);
    let request = screen.take_request().unwrap();
    assert!(matches!(request.kind, Kind::Status));
    assert!(screen.take_request().is_none(), "a request is taken once");
    assert_eq!(screen.deliver(Response::Status(set)), Step::Stay);
    screen
}

#[test]
fn a_key_session_sets_a_password_in_two_prompts_and_never_shows_it() {
    let mut screen = opened(LoginMethod::Key, false);
    screen.press(PasswordKey::Char('s'));
    type_text(&mut screen, GOOD);
    let shown = screen.view();
    assert_eq!(shown.fields[0].label, strings::PROMPT_NEW);
    assert_eq!(shown.fields[0].value, "*".repeat(GOOD.chars().count()));
    assert!(!format!("{shown:?}").contains(GOOD));
    screen.press(PasswordKey::Enter);
    assert_eq!(screen.view().fields[0].label, strings::PROMPT_REPEAT);
    type_text(&mut screen, GOOD);
    screen.press(PasswordKey::Enter);
    let request = screen.take_request().unwrap();
    assert_eq!(request.method, LoginMethod::Key);
    match request.kind {
        Kind::Set { current: None, new } => assert_eq!(new.chars(), GOOD.chars().count()),
        other => panic!("unexpected {other:?}"),
    }
    assert!(screen.is_waiting());
    // Keys are ignored while the answer is out.
    assert_eq!(screen.press(PasswordKey::Escape), Step::Stay);
    assert_eq!(screen.deliver(Response::Done(Done::Set)), Step::Stay);
    let page = screen.view();
    assert_eq!(page.notice.unwrap().text, strings::DONE_SET);
    assert!(
        page.menu.iter().any(|item| item.key == 'c'),
        "clear is now offered"
    );
}

#[test]
fn the_screen_refuses_a_short_or_mismatched_password_before_asking() {
    let mut screen = opened(LoginMethod::Key, false);
    screen.press(PasswordKey::Char('s'));
    type_text(&mut screen, "short");
    screen.press(PasswordKey::Enter);
    assert_eq!(screen.view().notice.unwrap().text, strings::ERR_SHORT);
    assert_eq!(screen.view().fields[0].label, strings::PROMPT_NEW);
    type_text(&mut screen, GOOD);
    screen.press(PasswordKey::Enter);
    type_text(&mut screen, OTHER);
    screen.press(PasswordKey::Enter);
    assert_eq!(screen.view().notice.unwrap().text, strings::ERR_MISMATCH);
    assert_eq!(screen.view().fields[0].label, strings::PROMPT_NEW);
    assert!(screen.take_request().is_none());
    screen.press(PasswordKey::Escape);
    assert!(screen.view().menu.iter().any(|item| item.key == 's'));
}

#[test]
fn a_password_session_is_asked_for_the_current_password_first() {
    let mut screen = opened(LoginMethod::Password, true);
    screen.press(PasswordKey::Char('s'));
    assert_eq!(screen.view().fields[0].label, strings::PROMPT_CURRENT);
    type_text(&mut screen, GOOD);
    screen.press(PasswordKey::Enter);
    type_text(&mut screen, OTHER);
    screen.press(PasswordKey::Enter);
    type_text(&mut screen, OTHER);
    screen.press(PasswordKey::Enter);
    match screen.take_request().unwrap().kind {
        Kind::Set {
            current: Some(_), ..
        } => {}
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(
        screen.deliver(Response::Refused(Refusal::WrongCurrent)),
        Step::Stay
    );
    assert_eq!(screen.view().notice.unwrap().text, strings::ERR_CURRENT);
    // Clearing asks for it too.
    screen.press(PasswordKey::Char('c'));
    assert_eq!(screen.view().fields[0].label, strings::PROMPT_CURRENT);
    type_text(&mut screen, GOOD);
    screen.press(PasswordKey::Enter);
    match screen.take_request().unwrap().kind {
        Kind::Clear { current: Some(_) } => {}
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn a_key_session_confirms_a_clear_and_a_denial_leaves() {
    let mut screen = opened(LoginMethod::Key, true);
    screen.press(PasswordKey::Char('c'));
    screen.press(PasswordKey::Char('n'));
    assert!(screen.take_request().is_none());
    screen.press(PasswordKey::Char('c'));
    screen.press(PasswordKey::Char('y'));
    assert!(matches!(
        screen.take_request().unwrap().kind,
        Kind::Clear { current: None }
    ));
    assert_eq!(
        screen.deliver(Response::Refused(Refusal::Denied)),
        Step::Denied
    );
    // Without a password there is nothing to clear.
    let mut none = opened(LoginMethod::Key, false);
    none.press(PasswordKey::Char('c'));
    assert!(none.take_request().is_none());
    assert!(none.view().menu.iter().all(|item| item.key != 'c'));
    // A status that fails leaves the screen.
    let mut broken = PasswordScreen::open(LoginMethod::Key);
    let _ = broken.take_request();
    assert_eq!(
        broken.deliver(Response::Refused(Refusal::Failed)),
        Step::Leave
    );
}

#[test]
fn typing_is_bounded_and_control_characters_are_dropped() {
    let mut screen = opened(LoginMethod::Key, false);
    screen.press(PasswordKey::Char('s'));
    type_text(&mut screen, &"x".repeat(MAX_CHARS + 50));
    screen.press(PasswordKey::Char('\u{1b}'));
    assert_eq!(screen.view().fields[0].value.chars().count(), MAX_CHARS);
    screen.press(PasswordKey::Backspace);
    assert_eq!(screen.view().fields[0].value.chars().count(), MAX_CHARS - 1);
}

// ---- inside the signed-in session ----

mod in_the_session {
    use super::*;
    use crate::icons::Tier;
    use crate::terminal_settings::{MemberEffect, MemberFlow, MemberInput, TerminalSettings};

    fn settings_and_password() -> Snapshot {
        holds(
            1,
            &[
                Permission::AccountEditSettings,
                Permission::AccountSetPassword,
            ],
        )
    }

    fn flow(access: Snapshot, password_session: bool) -> MemberFlow {
        let handle = sanitize_username("alice").unwrap();
        let flow = MemberFlow::new(
            &handle,
            access,
            None,
            TerminalSettings::default().with_tier(Tier::Ascii),
        );
        if password_session {
            flow.signed_in_with_password()
        } else {
            flow
        }
    }

    fn press(flow: &mut MemberFlow, keys: &str) {
        for c in keys.chars() {
            let effect = flow.input(match c {
                '\n' => MemberInput::Enter,
                '\u{1b}' => MemberInput::Escape,
                c => MemberInput::Char(c),
            });
            assert_eq!(effect, MemberEffect::Stay);
        }
    }

    /// Returns each key as its own text: these tests look at the password screens and the
    /// settings menu entry, whose labels are not string-table keys.
    struct Keys;
    impl crate::terminal_settings::strings::TextSource for Keys {
        fn text<'a>(&'a self, key: &'a str) -> &'a str {
            key
        }
    }

    fn lines(flow: &MemberFlow) -> String {
        let view = flow.view(&Keys);
        let mut all = vec![view.heading.to_owned()];
        all.extend(view.body);
        all.extend(
            view.menu
                .iter()
                .map(|item| format!("{} {}", item.key, item.label)),
        );
        all.extend(
            view.fields
                .iter()
                .map(|f| format!("{}: {}", f.label, f.value)),
        );
        all.join("\n")
    }

    #[test]
    fn settings_offers_the_password_screen_only_with_the_permission() {
        let mut with = flow(settings_and_password(), false);
        press(&mut with, "xs");
        assert!(lines(&with).contains("s Password"));
        press(&mut with, "s");
        let request = with.take_password_request().unwrap();
        assert!(matches!(request.kind, Kind::Status));
        assert!(with.take_password_request().is_none());
        with.deliver_password(Response::Status(false));
        assert!(lines(&with).contains("No password is set"));
        // Esc returns to the settings; Esc again to the menu.
        press(&mut with, "\u{1b}");
        assert!(lines(&with).contains("s Password"));

        let mut without = flow(holds(1, &[Permission::AccountEditSettings]), false);
        press(&mut without, "xs");
        assert!(!lines(&without).contains("Password"));
        press(&mut without, "s");
        assert!(without.take_password_request().is_none());
        assert!(!lines(&without).contains("No password is set"));
    }

    /// D94 (3): setting or changing one's own password over telnet is allowed, so the settings
    /// of a password session (the shape telnet gives its members) open the password screen, and
    /// it asks for the current password first.
    #[test]
    fn a_password_session_gets_the_password_screen_and_must_give_the_current_one() {
        let mut f = flow(settings_and_password(), true);
        press(&mut f, "xs");
        press(&mut f, "s");
        let request = f.take_password_request().unwrap();
        assert_eq!(request.method, LoginMethod::Password);
    }

    #[test]
    fn the_flow_hands_the_screen_the_login_method() {
        for (password_session, method) in [(false, LoginMethod::Key), (true, LoginMethod::Password)]
        {
            let mut f = flow(settings_and_password(), password_session);
            press(&mut f, "xss");
            let request = f.take_password_request().unwrap();
            assert_eq!(request.method, method);
        }
    }

    #[test]
    fn losing_the_permission_closes_the_screen_on_the_next_key() {
        let mut f = flow(settings_and_password(), false);
        press(&mut f, "xss");
        let _ = f.take_password_request();
        f.deliver_password(Response::Status(true));
        f.set_access(holds(1, &[Permission::AccountEditSettings]));
        press(&mut f, "c");
        assert!(f.take_password_request().is_none());
        assert!(!lines(&f).contains("Clear password"));
    }
}
