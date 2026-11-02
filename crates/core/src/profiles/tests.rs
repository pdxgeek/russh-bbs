#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::too_many_lines,
    clippy::unused_async_trait_impl,
    clippy::needless_pass_by_value,
    clippy::assert_is_empty,
    clippy::format_push_string
)]

use std::sync::Mutex;

use super::*;
use crate::access::{Permission, Slug, Snapshot, Via};
use crate::sanitize::sanitize_username;
use crate::store::{AccountId, StoreError};

fn id(n: i64) -> AccountId {
    AccountId::from_raw(n)
}

fn snapshot(account: i64, permissions: &[Permission]) -> Snapshot {
    Snapshot::from_role(
        Some(id(account)),
        &Slug::MEMBER,
        10,
        &Via::Account,
        permissions.iter().copied(),
    )
}

const MEMBER_SET: &[Permission] = &[
    Permission::MembersView,
    Permission::AccountEditProfile,
    Permission::AccountEditSettings,
];

/// An in-memory store. `state` is `active` or anything else; only active rows are visible.
#[derive(Default)]
struct Fake {
    rows: Mutex<Vec<(AccountId, String, bool, RawProfile)>>,
    calls: Mutex<u32>,
    saves: Mutex<u32>,
    audits: Mutex<Vec<crate::audit::AuditEvent>>,
}

impl Fake {
    fn add(&self, n: i64, handle: &str, active: bool) {
        self.rows
            .lock()
            .unwrap()
            .push((id(n), handle.to_owned(), active, RawProfile::default()));
    }

    fn set_raw(&self, n: i64, raw: RawProfile) {
        for row in self.rows.lock().unwrap().iter_mut() {
            if row.0 == id(n) {
                row.3 = raw.clone();
            }
        }
    }

    fn calls(&self) -> u32 {
        *self.calls.lock().unwrap()
    }
}

impl ProfileStore for Fake {
    async fn list_members(
        &self,
        query: &MemberQuery,
        limit: usize,
    ) -> Result<Vec<MemberSummary>, StoreError> {
        *self.calls.lock().unwrap() += 1;
        let mut rows: Vec<_> = self
            .rows
            .lock()
            .unwrap()
            .iter()
            .filter(|row| row.2)
            .filter(|row| {
                row.1
                    .to_ascii_lowercase()
                    .starts_with(query.prefix.as_str())
            })
            .filter(|row| {
                query
                    .after
                    .as_ref()
                    .is_none_or(|after| row.1.to_ascii_lowercase().as_str() > after.as_str())
            })
            .cloned()
            .collect();
        rows.sort_by_key(|row| row.1.to_ascii_lowercase());
        Ok(rows
            .into_iter()
            .take(limit)
            .map(|row| {
                MemberSummary::new(
                    row.0,
                    sanitize_username(&row.1).unwrap(),
                    &row.3.display_name,
                )
            })
            .collect())
    }

    async fn member(&self, who: AccountId) -> Result<Option<MemberRecord>, StoreError> {
        *self.calls.lock().unwrap() += 1;
        Ok(self
            .rows
            .lock()
            .unwrap()
            .iter()
            .find(|row| row.0 == who && row.2)
            .map(|row| MemberRecord {
                id: row.0,
                handle: sanitize_username(&row.1).unwrap(),
                profile: Profile::from_stored(&row.3),
            }))
    }

    async fn save_profile(
        &self,
        who: AccountId,
        profile: &Profile,
        audit: Option<&crate::audit::AuditEvent>,
    ) -> Result<(), StoreError> {
        *self.saves.lock().unwrap() += 1;
        if let Some(event) = audit {
            self.audits.lock().unwrap().push(event.clone());
        }
        for row in self.rows.lock().unwrap().iter_mut() {
            if row.0 == who {
                row.3 = profile.to_raw();
            }
        }
        Ok(())
    }
}

fn run<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(future)
}

// ---- fields -------------------------------------------------------------------------------

#[test]
fn every_field_is_capped_single_line_and_clean() {
    let long = "x".repeat(1_000);
    let raw = RawProfile {
        display_name: long.clone(),
        pronouns: long.clone(),
        location: long.clone(),
        about: long.clone(),
        website: long,
    };
    let profile = Profile::from_raw(&raw);
    for field in Field::ALL {
        assert_eq!(
            profile.get(field).chars().count(),
            field.max_chars(),
            "{field:?}"
        );
    }
}

#[test]
fn hostile_text_is_neutralized_in_every_field() {
    let hostile =
        "a\u{1b}[31mred\u{1b}]0;title\u{7}\nline\u{202e}b\u{200b}\u{e000}\u{27e6}\u{2299}\u{27e7}z";
    let raw = RawProfile {
        display_name: hostile.to_owned(),
        pronouns: hostile.to_owned(),
        location: hostile.to_owned(),
        about: hostile.to_owned(),
        website: hostile.to_owned(),
    };
    let profile = Profile::from_raw(&raw);
    for field in Field::ALL {
        let text = profile.get(field);
        assert!(!text.contains('\u{1b}'), "{field:?}: escape survived");
        assert!(!text.contains('\n') && !text.contains('\u{7}'), "{field:?}");
        assert!(!text.contains('\u{202e}') && !text.contains('\u{200b}'));
        for reserved in ['\u{e000}', '\u{27e6}', '\u{27e7}', '\u{2299}'] {
            assert!(!text.contains(reserved), "{field:?}: {reserved:?} survived");
        }
    }
    // The stored form is sanitized again when read, so a row edited by hand is no way in.
    let reread = Profile::from_stored(&profile.to_raw());
    assert_eq!(reread, profile);
    let dirty = Profile::from_stored(&RawProfile {
        about: "\u{1b}[2Jgone".to_owned(),
        ..RawProfile::default()
    });
    assert!(!dirty.get(Field::About).contains('\u{1b}'));
}

#[test]
fn the_website_is_plain_text_and_is_not_interpreted() {
    let profile = Profile::from_raw(&RawProfile {
        website: "  http://169.254.169.254/latest  ".to_owned(),
        ..RawProfile::default()
    });
    assert_eq!(profile.get(Field::Website), "http://169.254.169.254/latest");
}

#[test]
fn changed_from_names_fields_and_not_values() {
    let a = Profile::from_raw(&RawProfile {
        location: "Here".to_owned(),
        ..RawProfile::default()
    });
    let b = Profile::from_raw(&RawProfile {
        location: "There".to_owned(),
        about: "hi".to_owned(),
        ..RawProfile::default()
    });
    assert_eq!(b.changed_from(&a), vec![Field::Location, Field::About]);
    assert!(a.changed_from(&a).is_empty());
}

#[test]
fn handle_prefix_keeps_only_handle_characters() {
    assert_eq!(
        HandlePrefix::parse("Al%_ice-9 \u{1b}[").as_str(),
        "al_ice-9"
    );
    assert!(HandlePrefix::parse("%%").is_empty());
    assert_eq!(HandlePrefix::parse(&"a".repeat(100)).as_str().len(), 24);
}

// ---- service and permissions ---------------------------------------------------------------

fn seeded() -> Fake {
    let store = Fake::default();
    store.add(1, "alice", true);
    store.add(2, "bob", true);
    store.add(3, "pending_pat", false);
    store.add(4, "carol", true);
    store
}

#[test]
fn the_list_holds_active_members_only_in_handle_order() {
    let store = seeded();
    let access = snapshot(1, MEMBER_SET);
    let Response::Page(_, page) = run(execute(
        &store,
        &access,
        Request::List(MemberQuery::default()),
    )) else {
        panic!("not a page");
    };
    let handles: Vec<_> = page.members.iter().map(|m| m.handle.as_str()).collect();
    assert_eq!(handles, ["alice", "bob", "carol"]);
    assert!(page.next.is_none());
}

#[test]
fn paging_never_skips_or_repeats_and_search_filters_by_prefix() {
    let store = Fake::default();
    for n in 0..25 {
        store.add(n + 1, &format!("user{n:02}"), true);
    }
    let access = snapshot(1, MEMBER_SET);
    let mut seen = Vec::new();
    let mut query = MemberQuery::default();
    loop {
        let Response::Page(_, page) = run(execute(&store, &access, Request::List(query.clone())))
        else {
            panic!("not a page");
        };
        assert!(page.members.len() <= PAGE_SIZE);
        seen.extend(page.members.iter().map(|m| m.handle.as_str().to_owned()));
        match page.next {
            Some(next) => query.after = Some(next),
            None => break,
        }
    }
    let expected: Vec<String> = (0..25).map(|n| format!("user{n:02}")).collect();
    assert_eq!(seen, expected);

    let Response::Page(_, page) = run(execute(
        &store,
        &access,
        Request::List(MemberQuery {
            prefix: HandlePrefix::parse("USER1"),
            after: None,
        }),
    )) else {
        panic!("not a page");
    };
    assert_eq!(page.members.len(), 9, "user10..user18, then a next page");
    assert!(page.next.is_some());
}

#[test]
fn the_waiting_room_and_empty_snapshots_get_nothing_and_the_store_is_not_asked() {
    let store = seeded();
    let waiting = Snapshot::from_role(
        None,
        &Slug::WAITING,
        0,
        &Via::Account,
        [Permission::WaitingView],
    );
    for access in [&waiting, &Snapshot::empty()] {
        for request in [
            Request::List(MemberQuery::default()),
            Request::View(id(1)),
            Request::Edit(id(1)),
            Request::Save(id(1), RawProfile::default()),
        ] {
            assert_eq!(
                run(execute(&store, access, request.clone())),
                Response::NotPermitted,
                "{request:?}"
            );
        }
    }
    assert_eq!(store.calls(), 0, "a denied request never reaches the store");
}

#[test]
fn a_proof_for_another_permission_is_refused() {
    let store = seeded();
    let access = snapshot(1, MEMBER_SET);
    let settings = access
        .require(Permission::AccountEditSettings, None)
        .unwrap();
    assert!(matches!(
        run(list_members(&store, &settings, &MemberQuery::default())),
        Err(ProfileError::NotPermitted)
    ));
    assert!(matches!(
        run(view_profile(&store, &settings, id(1))),
        Err(ProfileError::NotPermitted)
    ));
    assert!(matches!(
        run(edit_profile(
            &store,
            &settings,
            id(1),
            &RawProfile::default()
        )),
        Err(ProfileError::NotPermitted)
    ));
    // The view proof does not edit either.
    let view = access.require(Permission::MembersView, None).unwrap();
    assert!(matches!(
        run(edit_profile(&store, &view, id(1), &RawProfile::default())),
        Err(ProfileError::NotPermitted)
    ));
}

#[test]
fn inactive_and_absent_members_look_the_same() {
    let store = seeded();
    let access = snapshot(1, MEMBER_SET);
    for target in [3, 99] {
        assert_eq!(
            run(execute(&store, &access, Request::View(id(target)))),
            Response::NotFound
        );
    }
    let staff = snapshot(1, &[Permission::MembersView, Permission::MembersEdit]);
    assert_eq!(
        run(execute(&store, &staff, Request::Edit(id(3)))),
        Response::NotFound,
        "staff cannot reach a pending account either"
    );
    assert_eq!(*store.saves.lock().unwrap(), 0);
}

#[test]
fn a_member_edits_their_own_profile_and_not_anyone_elses() {
    let store = seeded();
    let access = snapshot(1, MEMBER_SET);
    let raw = RawProfile {
        display_name: "Alice \u{1b}[1mA".to_owned(),
        location: "Portland".to_owned(),
        ..RawProfile::default()
    };
    let Response::Saved(saved) = run(execute(&store, &access, Request::Save(id(1), raw.clone())))
    else {
        panic!("not saved");
    };
    assert!(!saved.by_staff);
    assert_eq!(saved.changed, vec![Field::DisplayName, Field::Location]);
    assert!(!saved.profile.get(Field::DisplayName).contains('\u{1b}'));
    assert!(
        store.audits.lock().unwrap().is_empty(),
        "own edits are not audited"
    );

    assert_eq!(
        run(execute(&store, &access, Request::Save(id(2), raw.clone()))),
        Response::NotPermitted
    );
    assert_eq!(
        run(execute(&store, &access, Request::Edit(id(2)))),
        Response::NotPermitted
    );
    // Nothing was written to Bob.
    let Response::Profile(bob) = run(execute(&store, &access, Request::View(id(2)))) else {
        panic!("not a profile");
    };
    assert_eq!(bob.profile, Profile::default());
}

#[test]
fn staff_with_members_edit_edit_another_profile_and_it_is_flagged_for_audit() {
    let store = seeded();
    let staff = snapshot(1, &[Permission::MembersView, Permission::MembersEdit]);
    let raw = RawProfile {
        about: "edited by staff".to_owned(),
        ..RawProfile::default()
    };
    let Response::Saved(saved) = run(execute(&store, &staff, Request::Save(id(2), raw))) else {
        panic!("not saved");
    };
    assert!(saved.by_staff);
    {
        // The staff edit is audited with field names only; the owner's own edits are not.
        let audits = store.audits.lock().unwrap();
        assert_eq!(audits.len(), 1);
        assert_eq!(audits[0].action, crate::audit::Action::ProfileEdited);
        assert_eq!(audits[0].detail.as_str(), "fields about");
        assert!(!audits[0].detail.as_str().contains("edited by staff"));
    }
    assert_eq!(saved.target, id(2));
    assert_eq!(saved.changed, vec![Field::About]);
    // `members.edit` is about other people: without `account.edit_profile` staff cannot edit
    // their own, and there is no fallback between the two.
    assert_eq!(
        run(execute(
            &store,
            &staff,
            Request::Save(id(1), RawProfile::default())
        )),
        Response::NotPermitted
    );
    assert_eq!(
        run(execute(&store, &staff, Request::Edit(id(1)))),
        Response::NotPermitted
    );
}

#[test]
fn saving_what_did_not_change_writes_nothing() {
    let store = seeded();
    store.set_raw(
        1,
        RawProfile {
            location: "Here".to_owned(),
            ..RawProfile::default()
        },
    );
    let access = snapshot(1, MEMBER_SET);
    let Response::Saved(saved) = run(execute(
        &store,
        &access,
        Request::Save(
            id(1),
            RawProfile {
                location: " Here ".to_owned(),
                ..RawProfile::default()
            },
        ),
    )) else {
        panic!("not saved");
    };
    assert!(saved.changed.is_empty());
    assert_eq!(*store.saves.lock().unwrap(), 0);
}

// ---- the screens ---------------------------------------------------------------------------

fn text_of(view: &ScreenView) -> String {
    let mut out = String::new();
    for line in &view.lines {
        match line {
            Line::Blank => {}
            Line::Heading(k) | Line::Text(k) => out.push_str(k),
            Line::Filled { key, values } => {
                out.push_str(key);
                for (_, v) in values {
                    out.push(' ');
                    out.push_str(v);
                }
            }
            Line::Pair { label, value } => {
                out.push_str(label);
                out.push(' ');
                out.push_str(value.as_deref().unwrap_or("-"));
            }
            Line::Row { key, handle, name } => out.push_str(&format!("[{key}] {handle} {name}")),
            Line::Field { label, value, .. } => out.push_str(&format!("{label}={value}")),
            Line::Notice { text, .. } => out.push_str(text),
        }
        out.push('\n');
    }
    out.push_str(view.footer);
    out
}

/// Run the screen's outstanding request against the store, as the front end does.
fn pump(screen: &mut MembersScreen, store: &Fake, access: &Snapshot) -> Step {
    let mut step = Step::Stay;
    while let Some(request) = screen.take_request() {
        step = screen.deliver(run(execute(store, access, request)));
    }
    step
}

fn press(screen: &mut MembersScreen, store: &Fake, access: &Snapshot, keys: &str) {
    for c in keys.chars() {
        let key = match c {
            '\r' => ScreenKey::Enter,
            '\u{8}' => ScreenKey::Backspace,
            '\u{1b}' => ScreenKey::Escape,
            c => ScreenKey::Char(c),
        };
        screen.press(access, key);
        pump(screen, store, access);
    }
}

#[test]
fn the_list_screen_pages_searches_opens_a_profile_and_goes_back() {
    let store = Fake::default();
    for n in 0..12 {
        store.add(n + 1, &format!("user{n:02}"), true);
    }
    store.set_raw(
        1,
        RawProfile {
            display_name: "Zero".to_owned(),
            ..RawProfile::default()
        },
    );
    let access = snapshot(1, MEMBER_SET);
    let mut screen = MembersScreen::list(Some(id(1)));
    pump(&mut screen, &store, &access);
    let first = text_of(&screen.view(&access));
    assert!(
        first.contains("[1] user00 Zero") && first.contains("[9] user08"),
        "{first}"
    );
    assert!(!first.contains("user09"));

    press(&mut screen, &store, &access, "n");
    let second = text_of(&screen.view(&access));
    assert!(
        second.contains("[1] user09") && second.contains("[3] user11"),
        "{second}"
    );

    press(&mut screen, &store, &access, "p");
    assert!(text_of(&screen.view(&access)).contains("[1] user00"));

    // Search: `/`, type, Enter. Backspace edits; characters a handle cannot hold are ignored.
    press(&mut screen, &store, &access, "/user1x\u{8}%\r");
    let found = text_of(&screen.view(&access));
    assert!(
        found.contains("[1] user10") && found.contains("[2] user11"),
        "{found}"
    );
    assert!(!found.contains("user00"));
    press(&mut screen, &store, &access, "/zzz\r");
    assert!(text_of(&screen.view(&access)).contains(keys::LIST_NO_MATCH));
    press(&mut screen, &store, &access, "c");
    assert!(text_of(&screen.view(&access)).contains("[1] user00"));

    // Open the first row and come back to the same list.
    press(&mut screen, &store, &access, "1");
    let profile = text_of(&screen.view(&access));
    assert!(
        profile.contains("user00") && profile.contains("Zero"),
        "{profile}"
    );
    press(&mut screen, &store, &access, "\u{1b}");
    assert!(text_of(&screen.view(&access)).contains("[1] user00"));
    // Esc on the list leaves.
    assert_eq!(screen.press(&access, ScreenKey::Escape), Step::Leave);
}

#[test]
fn keys_are_ignored_while_a_request_is_outstanding() {
    let store = seeded();
    let access = snapshot(1, MEMBER_SET);
    let mut screen = MembersScreen::list(Some(id(1)));
    // No answer yet: a digit does nothing, even though it would be the first row.
    assert_eq!(screen.press(&access, ScreenKey::Char('1')), Step::Stay);
    let request = screen.take_request().unwrap();
    assert!(
        screen.take_request().is_none(),
        "a request is handed out once"
    );
    assert_eq!(screen.press(&access, ScreenKey::Char('n')), Step::Stay);
    screen.deliver(run(execute(&store, &access, request)));
    assert!(text_of(&screen.view(&access)).contains("[1] alice"));
}

#[test]
fn the_edit_form_prefills_edits_field_by_field_and_saves_on_the_last_enter() {
    let store = seeded();
    store.set_raw(
        1,
        RawProfile {
            display_name: "Alice".to_owned(),
            pronouns: "she/her".to_owned(),
            ..RawProfile::default()
        },
    );
    let access = snapshot(1, MEMBER_SET);
    let mut screen = MembersScreen::edit_own(id(1));
    pump(&mut screen, &store, &access);
    let form = text_of(&screen.view(&access));
    // The honest statement and the plain-text note are on the form; nothing says "private".
    assert!(form.contains(keys::EDIT_READABLE) && form.contains(keys::EDIT_PLAIN_TEXT));
    assert!(form.contains("members.label.display_name=Alice"), "{form}");

    // Enter keeps "Alice"; Enter keeps pronouns; type a location; skip about; type a website.
    press(&mut screen, &store, &access, "\r\r");
    press(&mut screen, &store, &access, "Paris\r");
    press(&mut screen, &store, &access, "\r");
    press(&mut screen, &store, &access, "example.org\r");
    let saved = text_of(&screen.view(&access));
    assert!(saved.contains(keys::NOTICE_SAVED), "{saved}");
    assert!(
        saved.contains("Paris") && saved.contains("example.org"),
        "{saved}"
    );
    let Response::Profile(now) = run(execute(&store, &access, Request::View(id(1)))) else {
        panic!("not a profile");
    };
    assert_eq!(now.profile.get(Field::Location), "Paris");
    assert_eq!(now.profile.get(Field::DisplayName), "Alice");

    // Esc from the view of an edit opened from the menu leaves.
    assert_eq!(screen.press(&access, ScreenKey::Escape), Step::Leave);
}

#[test]
fn escape_discards_an_edit_and_a_field_stops_at_its_cap() {
    let store = seeded();
    let access = snapshot(1, MEMBER_SET);
    let mut screen = MembersScreen::edit_own(id(1));
    pump(&mut screen, &store, &access);
    press(&mut screen, &store, &access, &"x".repeat(100));
    let form = text_of(&screen.view(&access));
    assert!(form.contains(&format!("display_name={}\n", "x".repeat(DISPLAY_NAME_MAX))));
    assert_eq!(screen.press(&access, ScreenKey::Escape), Step::Leave);
    assert_eq!(*store.saves.lock().unwrap(), 0, "nothing was written");
}

#[test]
fn the_edit_key_is_offered_only_where_an_edit_would_be_allowed() {
    let store = seeded();
    let member = snapshot(1, MEMBER_SET);
    let mut screen = MembersScreen::list(Some(id(1)));
    pump(&mut screen, &store, &member);
    // Bob's profile: a plain member cannot edit it.
    press(&mut screen, &store, &member, "2");
    assert_eq!(screen.view(&member).footer, keys::VIEW_FOOTER);
    press(&mut screen, &store, &member, "e");
    assert!(
        screen.take_request().is_none(),
        "the key does nothing without the permission"
    );
    // Own profile: the key is offered.
    press(&mut screen, &store, &member, "\u{1b}");
    press(&mut screen, &store, &member, "1");
    assert_eq!(screen.view(&member).footer, keys::VIEW_FOOTER_EDIT);

    // Staff see it on Bob's profile too, and the form says whose profile it is.
    let staff = snapshot(1, &[Permission::MembersView, Permission::MembersEdit]);
    press(&mut screen, &store, &staff, "\u{1b}");
    press(&mut screen, &store, &staff, "2");
    assert_eq!(screen.view(&staff).footer, keys::VIEW_FOOTER_EDIT);
    press(&mut screen, &store, &staff, "e");
    let form = screen.view(&staff);
    assert!(
        form.lines.iter().any(|line| matches!(
            line,
            Line::Filled { key, values } if *key == keys::EDIT_TITLE_OTHER
                && values == &vec![("handle", "bob".to_owned())]
        )),
        "{form:?}"
    );
}

#[test]
fn a_permission_lost_mid_screen_is_caught_by_the_request() {
    let store = seeded();
    let member = snapshot(1, MEMBER_SET);
    let mut screen = MembersScreen::list(Some(id(1)));
    pump(&mut screen, &store, &member);
    // The role changed: the next request is checked against the new snapshot.
    let gone = snapshot(1, &[]);
    screen.press(&gone, ScreenKey::Char('n'));
    screen.press(&gone, ScreenKey::Char('1'));
    let request = screen.take_request().unwrap();
    let step = screen.deliver(run(execute(&store, &gone, request)));
    assert_eq!(step, Step::Denied);
}

#[test]
fn rows_from_a_hostile_store_are_sanitized_before_they_reach_a_screen() {
    let store = seeded();
    store.set_raw(
        1,
        RawProfile {
            display_name: "\u{1b}[2J\u{27e6}\u{2299}\u{27e7}evil".to_owned(),
            ..RawProfile::default()
        },
    );
    let access = snapshot(2, MEMBER_SET);
    let mut screen = MembersScreen::list(Some(id(2)));
    pump(&mut screen, &store, &access);
    let shown = text_of(&screen.view(&access));
    assert!(
        !shown.contains('\u{1b}') && !shown.contains('\u{27e6}'),
        "{shown:?}"
    );
    assert!(shown.contains("evil"));
}
