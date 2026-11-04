#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::too_many_lines,
    clippy::unused_async_trait_impl,
    clippy::assert_is_empty
)]

use std::collections::HashMap;
use std::sync::Mutex;

use super::*;
use crate::access::{Fingerprint, Permission, Slug, Snapshot, Via};
use crate::audit::{Action, AuditEvent};
use crate::passwords::{Inline, Secret};
use crate::sanitize::sanitize_username;
use crate::store::{Account, AccountId, AccountState, StoreError, UnixSeconds};

const KEY_A: &str =
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

fn id(n: i64) -> AccountId {
    AccountId::from_raw(n)
}

fn fingerprint(c: char) -> Fingerprint {
    Fingerprint::parse(&format!("SHA256:{}", c.to_string().repeat(43))).unwrap()
}

fn holds(n: i64, rank: u8, permissions: &[Permission]) -> Snapshot {
    Snapshot::from_role(
        Some(id(n)),
        &Slug::MEMBER,
        rank,
        &Via::Account,
        permissions.iter().copied(),
    )
}

fn member(n: i64) -> Snapshot {
    holds(n, 10, &[Permission::KeysManage])
}

fn staff(n: i64, rank: u8) -> Snapshot {
    holds(n, rank, &[Permission::KeysRecover])
}

fn waiting() -> Snapshot {
    Snapshot::from_role(
        None,
        &Slug::WAITING,
        0,
        &Via::Waiting,
        [Permission::KeysRedeem],
    )
}

fn account(n: i64, handle: &str, state: AccountState) -> Account {
    Account {
        id: id(n),
        username: sanitize_username(handle).unwrap(),
        state,
        created_at: UnixSeconds::from_raw(0),
    }
}

/// A store that answers as scripted and records what it was asked.
#[derive(Default)]
struct Fake {
    accounts: Vec<(Account, u8)>,
    keys: Mutex<Vec<Fingerprint>>,
    taken: Vec<Fingerprint>,
    last_way_in: bool,
    rate_limited: bool,
    code_ok: bool,
    audits: Mutex<Vec<AuditEvent>>,
    issued: Mutex<Vec<NewCode>>,
    redeemed_with: Mutex<HashMap<String, String>>,
}

impl KeyStore for Fake {
    async fn list_keys(&self, _account: AccountId) -> Result<Vec<KeyRecord>, StoreError> {
        Ok(self
            .keys
            .lock()
            .unwrap()
            .iter()
            .map(|fingerprint| KeyRecord {
                fingerprint: fingerprint.clone(),
                added_at: UnixSeconds::from_raw(1),
                last_used_at: None,
            })
            .collect())
    }

    async fn add_key(
        &self,
        _account: AccountId,
        fingerprint: &Fingerprint,
        audit: &AuditEvent,
    ) -> Result<(), StoreError> {
        if self.taken.contains(fingerprint) {
            return Err(StoreError::FingerprintTaken);
        }
        self.keys.lock().unwrap().push(fingerprint.clone());
        self.audits.lock().unwrap().push(audit.clone());
        Ok(())
    }

    async fn remove_key(
        &self,
        _account: AccountId,
        fingerprint: &Fingerprint,
        audit: &AuditEvent,
    ) -> Result<RemoveOutcome, StoreError> {
        if self.last_way_in {
            return Ok(RemoveOutcome::LastWayIn);
        }
        let mut keys = self.keys.lock().unwrap();
        let Some(at) = keys.iter().position(|k| k == fingerprint) else {
            return Ok(RemoveOutcome::NotFound);
        };
        keys.remove(at);
        self.audits.lock().unwrap().push(audit.clone());
        Ok(RemoveOutcome::Removed)
    }

    async fn note_key_use(&self, _fingerprint: &Fingerprint) -> Result<(), StoreError> {
        Ok(())
    }

    async fn take_notices(&self, _account: AccountId) -> Result<Vec<KeyNotice>, StoreError> {
        Err(StoreError::Corrupt("no".to_owned()))
    }

    async fn active_account(&self, handle: &str) -> Result<Option<Account>, StoreError> {
        Ok(self
            .accounts
            .iter()
            .find(|(a, _)| a.username.as_str().eq_ignore_ascii_case(handle))
            .filter(|(a, _)| a.state == AccountState::Active)
            .map(|(a, _)| a.clone()))
    }

    async fn account_rank(&self, account: AccountId) -> Result<u8, StoreError> {
        Ok(self
            .accounts
            .iter()
            .find(|(a, _)| a.id == account)
            .map_or(0, |(_, rank)| *rank))
    }

    async fn issue_code(
        &self,
        code: &NewCode,
        audit: &AuditEvent,
    ) -> Result<UnixSeconds, IssueError> {
        if self.rate_limited {
            return Err(IssueError::RateLimited {
                retry_at: UnixSeconds::from_raw(99),
            });
        }
        self.issued.lock().unwrap().push(code.clone());
        self.audits.lock().unwrap().push(audit.clone());
        Ok(UnixSeconds::from_raw(1_000 + code.ttl_secs))
    }

    async fn redeem_link_code(
        &self,
        hash: &LoginCodeHash,
        fingerprint: &Fingerprint,
    ) -> Result<Account, RedeemError> {
        self.redeemed_with
            .lock()
            .unwrap()
            .insert(hash.to_hex(), fingerprint.to_string());
        if self.code_ok {
            Ok(account(1, "alice", AccountState::Active))
        } else {
            Err(RedeemError::Refused(RedeemRefusal::Code))
        }
    }

    async fn redeem_recovery_code(
        &self,
        hash: &LoginCodeHash,
        fingerprint: &Fingerprint,
        password_hash: &str,
    ) -> Result<Account, RedeemError> {
        assert!(password_hash.starts_with("$argon2id$"));
        self.redeemed_with
            .lock()
            .unwrap()
            .insert(hash.to_hex(), fingerprint.to_string());
        if self.code_ok {
            Ok(account(1, "alice", AccountState::Active))
        } else {
            Err(RedeemError::Refused(RedeemRefusal::Key))
        }
    }
}

#[tokio::test]
async fn my_keys_needs_the_permission_and_touches_nothing_without_it() {
    let store = Fake::default();
    let nothing = holds(1, 10, &[]);
    assert!(matches!(
        list_keys(&store, &nothing).await,
        Err(KeyError::Denied(_))
    ));
    assert!(matches!(
        add_key(&store, &nothing, KEY_A).await,
        Err(KeyError::Denied(_))
    ));
    assert!(matches!(
        remove_key(&store, &nothing, &fingerprint('a')).await,
        Err(KeyError::Denied(_))
    ));
    assert!(matches!(
        make_link_code(&store, &nothing, &[0; 10]).await,
        Err(KeyError::Denied(_))
    ));
    assert!(store.audits.lock().unwrap().is_empty());
    assert!(store.issued.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_pasted_key_is_validated_and_a_taken_key_has_one_generic_refusal() {
    let added = parse_ed25519(KEY_A).unwrap();
    let store = Fake {
        taken: vec![added.clone()],
        ..Fake::default()
    };
    // Bound to another account, or to this one: the same refusal.
    assert!(matches!(
        add_key(&store, &member(1), KEY_A).await,
        Err(KeyError::CannotAdd)
    ));
    for bad in ["", "ssh-rsa AAAA", "-----BEGIN OPENSSH PRIVATE KEY-----"] {
        assert!(matches!(
            add_key(&store, &member(1), bad).await,
            Err(KeyError::NotAKey(_))
        ));
    }
    let fresh = Fake::default();
    let got = add_key(&fresh, &member(1), KEY_A).await.unwrap();
    assert_eq!(got, added);
    let audits = fresh.audits.lock().unwrap();
    assert_eq!(audits.len(), 1);
    assert_eq!(audits.first().unwrap().action, Action::KeyAdded);
}

#[tokio::test]
async fn removal_reports_the_last_way_in_and_a_missing_key() {
    let store = Fake {
        keys: Mutex::new(vec![fingerprint('a')]),
        ..Fake::default()
    };
    assert!(matches!(
        remove_key(&store, &member(1), &fingerprint('b')).await,
        Err(KeyError::NoSuchKey)
    ));
    remove_key(&store, &member(1), &fingerprint('a'))
        .await
        .unwrap();
    let locked = Fake {
        last_way_in: true,
        ..Fake::default()
    };
    assert!(matches!(
        remove_key(&locked, &member(1), &fingerprint('a')).await,
        Err(KeyError::LastWayIn)
    ));
    assert!(locked.audits.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_link_code_is_stored_hashed_with_its_ttl_and_limit() {
    let store = Fake::default();
    let random = random_bytes(CodeKind::Link).unwrap();
    let issued = make_link_code(&store, &member(7), &random).await.unwrap();
    let new = store.issued.lock().unwrap().first().unwrap().clone();
    assert_eq!(new.kind, CodeKind::Link);
    assert_eq!(new.target, id(7));
    assert_eq!(new.issuer, id(7));
    assert_eq!(new.ttl_secs, 600);
    assert_eq!(new.per_hour, Some(LINK_CODES_PER_HOUR));
    assert_eq!(new.hash, issued.code.hash());
    // The audit row names the account and never the code.
    let audit = format!("{:?}", store.audits.lock().unwrap());
    assert!(!audit.contains(issued.code.reveal()));
    assert!(!audit.contains(&issued.code.hash().to_hex()));

    let limited = Fake {
        rate_limited: true,
        ..Fake::default()
    };
    assert!(matches!(
        make_link_code(&limited, &member(7), &random).await,
        Err(KeyError::RateLimited { .. })
    ));
}

#[tokio::test]
async fn staff_help_only_members_strictly_below_their_rank() {
    let store = Fake {
        accounts: vec![
            (account(1, "boss", AccountState::Active), 100),
            (account(2, "deputy", AccountState::Active), 80),
            (account(3, "other_deputy", AccountState::Active), 80),
            (account(4, "newbie", AccountState::Active), 10),
            (account(5, "gone", AccountState::Suspended), 10),
        ],
        ..Fake::default()
    };
    let random = random_bytes(CodeKind::Recovery).unwrap();
    // A cosysop (80) cannot help a sysop (100), an equal, or themselves.
    for handle in ["boss", "other_deputy", "deputy", "nobody", "gone", ""] {
        assert!(
            matches!(
                issue_recovery_code(&store, &staff(2, 80), handle, &random).await,
                Err(KeyError::NotYours)
            ),
            "{handle}"
        );
    }
    // A sysop cannot help another sysop or themselves, but can help a cosysop and a member.
    assert!(matches!(
        issue_recovery_code(&store, &staff(1, 100), "boss", &random).await,
        Err(KeyError::NotYours)
    ));
    for handle in ["deputy", "NEWBIE"] {
        let issued = issue_recovery_code(&store, &staff(1, 100), handle, &random)
            .await
            .unwrap();
        assert_eq!(issued.code.kind(), CodeKind::Recovery);
    }
    let recorded = store.issued.lock().unwrap();
    assert_eq!(recorded.len(), 2);
    assert!(
        recorded
            .iter()
            .all(|c| c.issuer == id(1) && c.ttl_secs == 86_400)
    );
    let audits = store.audits.lock().unwrap();
    assert!(
        audits
            .iter()
            .all(|a| a.action == Action::RecoveryCodeIssued)
    );
}

#[tokio::test]
async fn recovery_needs_its_own_permission() {
    let store = Fake {
        accounts: vec![(account(4, "newbie", AccountState::Active), 10)],
        ..Fake::default()
    };
    let random = random_bytes(CodeKind::Recovery).unwrap();
    // A member holding only keys.manage, or a high rank without the permission, is refused.
    for access in [member(1), holds(1, 100, &[Permission::KeysManage])] {
        assert!(matches!(
            issue_recovery_code(&store, &access, "newbie", &random).await,
            Err(KeyError::Denied(_))
        ));
    }
    assert!(store.issued.lock().unwrap().is_empty());
}

#[tokio::test]
async fn every_redemption_failure_is_the_same_refusal() {
    let secret = Secret::new("a long enough passphrase".to_owned());
    let fp = fingerprint('z');
    let refusing = Fake::default();
    assert_eq!(
        redeem_link_code(&refusing, &waiting(), "ABCDE-FGHJK", &fp)
            .await
            .unwrap_err(),
        Refused
    );
    assert_eq!(
        redeem_recovery_code(&refusing, &Inline, &waiting(), "ABCD", &fp, &secret)
            .await
            .unwrap_err(),
        Refused
    );
    // Without the permission the store is never asked.
    let nobody = holds(1, 0, &[]);
    assert_eq!(
        redeem_link_code(&refusing, &nobody, "x", &fp)
            .await
            .unwrap_err(),
        Refused
    );
    // A weak password is the same refusal, and also never reaches the store.
    let weak = Secret::new("short".to_owned());
    let accepting = Fake {
        code_ok: true,
        ..Fake::default()
    };
    assert_eq!(
        redeem_recovery_code(&accepting, &Inline, &waiting(), "ABCD", &fp, &weak)
            .await
            .unwrap_err(),
        Refused
    );
    assert!(accepting.redeemed_with.lock().unwrap().is_empty());
    // A good one works, and the typed code is looked up by its kind's hash.
    let account = redeem_recovery_code(&accepting, &Inline, &waiting(), "ABCD", &fp, &secret)
        .await
        .unwrap();
    assert_eq!(account.id, id(1));
    let looked_up = accepting.redeemed_with.lock().unwrap();
    assert!(looked_up.contains_key(&LoginCodeHash::of(CodeKind::Recovery, "abcd").to_hex()));
}

#[tokio::test]
async fn a_failed_notice_read_shows_nothing_and_does_not_fail_the_sign_in() {
    let store = Fake::default();
    assert!(notices_for_sign_in(&store, id(1)).await.is_empty());
}

#[test]
fn notices_are_worded_with_a_date_and_say_what_to_do() {
    let lines = notice_lines(&[
        KeyNotice {
            kind: NoticeKind::KeyAdded,
            at: UnixSeconds::from_raw(0),
            fingerprint: Some(fingerprint('a')),
        },
        KeyNotice {
            kind: NoticeKind::Recovered,
            at: UnixSeconds::from_raw(86_400),
            fingerprint: Some(fingerprint('b')),
        },
    ]);
    assert!(lines.first().unwrap().contains("Changes to your keys"));
    assert!(lines[1].starts_with("A key was added on 1970-01-01 00:00 UTC"));
    assert!(lines[2].contains("recovery code was used on 1970-01-02"));
    assert!(lines.last().unwrap().contains("If this was not you"));
    assert!(notice_lines(&[]).is_empty());
}

#[test]
fn the_keys_screen_never_shows_a_code_after_the_next_key() {
    use super::screen::{KeysKey, KeysScreen};
    let mut screen = KeysScreen::open(false);
    assert!(matches!(screen.take_request(), Some(Request::List)));
    screen.deliver(Response::Keys {
        keys: Vec::new(),
        changed: None,
    });
    screen.press(KeysKey::Char('l'));
    let Some(Request::LinkCode) = screen.take_request() else {
        panic!("the link key asks for a code");
    };
    let code = LoginCode::from_bytes(CodeKind::Link, &[5; 10]);
    let text = code.reveal().to_owned();
    screen.deliver(Response::Code(ShownCode {
        code,
        expires_at: UnixSeconds::from_raw(600),
    }));
    let shown = screen.view().body.join("\n");
    assert!(shown.contains(&text));
    screen.press(KeysKey::Enter);
    assert!(!screen.view().body.join("\n").contains(&text));
}

#[test]
fn the_keys_screen_lists_keys_and_removes_by_number() {
    use super::screen::{KeysKey, KeysScreen};
    let mut screen = KeysScreen::open(false);
    screen.take_request();
    let keys = vec![
        KeyRecord {
            fingerprint: fingerprint('a'),
            added_at: UnixSeconds::from_raw(0),
            last_used_at: None,
        },
        KeyRecord {
            fingerprint: fingerprint('b'),
            added_at: UnixSeconds::from_raw(0),
            last_used_at: Some(UnixSeconds::from_raw(3_600)),
        },
    ];
    screen.deliver(Response::Keys {
        keys,
        changed: None,
    });
    let body = screen.view().body.join("\n");
    assert!(body.contains("1. SHA256:aaaa"));
    assert!(body.contains("added 1970-01-01 00:00 UTC, last used never"));
    assert!(body.contains("last used 1970-01-01 01:00 UTC"));
    screen.press(KeysKey::Char('r'));
    screen.press(KeysKey::Char('2'));
    assert!(screen.view().body.join("").contains("SHA256:bbbb"));
    screen.press(KeysKey::Char('y'));
    let Some(Request::Remove(fp)) = screen.take_request() else {
        panic!("y confirms the removal");
    };
    assert_eq!(fp, fingerprint('b'));
    // Out of range numbers and cancel do nothing.
    let mut again = KeysScreen::open(false);
    again.take_request();
    again.deliver(Response::Keys {
        keys: vec![KeyRecord {
            fingerprint: fingerprint('a'),
            added_at: UnixSeconds::from_raw(0),
            last_used_at: None,
        }],
        changed: None,
    });
    again.press(KeysKey::Char('r'));
    again.press(KeysKey::Char('5'));
    again.press(KeysKey::Char('0'));
    assert!(again.take_request().is_none());
}
