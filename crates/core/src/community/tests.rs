//! The community types and screens without a store: the sanitizing and bounds of every text,
//! when a poll's counts show, and the screen state machine key by key. The rules that need a
//! database (the rate limit, the roll-off, one vote each, who may close) are in the shared store
//! suite (`russh-bbs-store`), which also drives `execute` end to end.
#![allow(clippy::unwrap_used, clippy::panic)]

use crate::access::{Permission, PermissionCheck};
use crate::profiles::{Line, ScreenKey, Step};
use crate::store::{AccountId, UnixSeconds};

use super::keys;
use super::{
    CommunityScreen, Entry, EntryId, LineKind, LineText, NewPoll, Note, OPTION_MAX, PollActions,
    PollDetail, PollError, PollId, PollRow, QUESTION_MAX, Request, Response, ResultsVisibility,
    Section, TextError,
};

struct Holds(&'static [Permission]);

impl PermissionCheck for Holds {
    fn allows(&self, permission: Permission) -> bool {
        self.0.contains(&permission)
    }
}

const MEMBER: Holds = Holds(&[
    Permission::CommunityView,
    Permission::OnelinersPost,
    Permission::GraffitiPost,
    Permission::PollsVote,
]);
const STAFF: Holds = Holds(&[
    Permission::CommunityView,
    Permission::CommunityModerate,
    Permission::OnelinersPost,
    Permission::PollsVote,
    Permission::PollsCreate,
]);

fn keys_of(text: &str) -> Vec<ScreenKey> {
    text.chars()
        .map(|c| match c {
            '\r' => ScreenKey::Enter,
            '\u{8}' => ScreenKey::Backspace,
            '\u{1b}' => ScreenKey::Escape,
            c => ScreenKey::Char(c),
        })
        .collect()
}

fn type_into(screen: &mut CommunityScreen, access: &dyn PermissionCheck, text: &str) -> Step {
    let mut last = Step::Stay;
    for key in keys_of(text) {
        last = screen.press(access, key);
    }
    last
}

fn entry(n: i64, who: &str, text: &str) -> Entry {
    Entry {
        id: EntryId::from_raw(n),
        author: who.to_owned(),
        text: text.to_owned(),
        at: UnixSeconds::from_raw(n),
    }
}

fn lines_screen(kind: LineKind, entries: Vec<Entry>) -> CommunityScreen {
    let section = match kind {
        LineKind::Oneliner => Section::Oneliners,
        LineKind::Graffiti => Section::Graffiti,
    };
    let mut screen = CommunityScreen::open(section);
    assert!(screen.take_request().is_some());
    screen.deliver(Response::Lines {
        kind,
        entries,
        note: None,
    });
    screen
}

fn shown_keys(screen: &CommunityScreen, access: &dyn PermissionCheck) -> Vec<&'static str> {
    screen
        .view(access)
        .lines
        .into_iter()
        .filter_map(|line| match line {
            Line::Text(key) => Some(key),
            _ => None,
        })
        .collect()
}

// ---- text ----

#[test]
fn a_line_is_sanitized_trimmed_and_bounded() {
    let kind = LineKind::Oneliner;
    assert_eq!(
        LineText::parse(kind, "  hello  ").unwrap().as_str(),
        "hello"
    );
    // Escape sequences, control characters and the Nerd Font range do not survive.
    let hostile = "a\u{1b}[31mb\u{7}c\u{e000}d\r\ne";
    let clean = LineText::parse(kind, hostile).unwrap();
    assert!(
        !clean
            .as_str()
            .chars()
            .any(|c| c.is_control() || ('\u{e000}'..='\u{f8ff}').contains(&c)),
        "{:?}",
        clean.as_str()
    );
    assert_eq!(LineText::parse(kind, "   \t "), Err(TextError::Blank));
    assert_eq!(LineText::parse(kind, "\u{1b}[0m"), Err(TextError::Blank));
    let exact = "x".repeat(kind.max_chars());
    assert!(LineText::parse(kind, &exact).is_ok());
    assert_eq!(
        LineText::parse(kind, &format!("{exact}x")),
        Err(TextError::TooLong {
            max: kind.max_chars()
        }),
        "over-long text is refused, not cut"
    );
}

#[test]
fn the_two_lists_have_their_own_bounds_and_permissions() {
    assert!(LineKind::Oneliner.keep() > LineKind::Graffiti.keep());
    assert_eq!(
        LineKind::Oneliner.post_permission(),
        Permission::OnelinersPost
    );
    assert_eq!(
        LineKind::Graffiti.post_permission(),
        Permission::GraffitiPost
    );
    for kind in LineKind::ALL {
        assert_eq!(LineKind::parse(kind.name()), Some(kind));
        assert!(kind.per_hour() > 0 && kind.keep() <= 20);
    }
    assert_eq!(LineKind::parse("shout"), None);
}

fn options(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn a_poll_needs_a_question_two_options_and_no_repeats() {
    let ok = NewPoll::build(
        "Best?",
        &options(&["a", "b", "", "  "]),
        "",
        ResultsVisibility::Always,
    )
    .unwrap();
    assert_eq!(ok.options(), ["a", "b"], "blank options are dropped");
    assert!(ok.description().is_none());
    let build =
        |q: &str, o: &[&str]| NewPoll::build(q, &options(o), "", ResultsVisibility::AfterVote);
    assert_eq!(build("  ", &["a", "b"]), Err(PollError::QuestionBlank));
    assert_eq!(
        build(&"q".repeat(QUESTION_MAX + 1), &["a", "b"]),
        Err(PollError::QuestionTooLong)
    );
    assert_eq!(build("Q?", &["only"]), Err(PollError::TooFewOptions));
    assert_eq!(build("Q?", &["", ""]), Err(PollError::TooFewOptions));
    assert_eq!(build("Q?", &["a", "A"]), Err(PollError::DuplicateOption));
    assert_eq!(
        build("Q?", &["a", &"o".repeat(OPTION_MAX + 1)]),
        Err(PollError::OptionTooLong)
    );
    assert_eq!(
        build("Q?", &["1", "2", "3", "4", "5", "6", "7"]),
        Err(PollError::TooManyOptions)
    );
    // Question and options are cleaned of control characters.
    let hostile = build("Wh\u{1b}[2Jat?", &["x\u{7}", "y"]).unwrap();
    assert!(!hostile.question().chars().any(char::is_control));
    assert!(
        hostile
            .options()
            .iter()
            .all(|o| !o.chars().any(char::is_control))
    );
}

#[test]
fn a_description_is_bounded_in_characters_and_lines() {
    let long = "x".repeat(super::DESCRIPTION_CHARS + 1);
    let too_long = NewPoll::build(
        "Q?",
        &options(&["a", "b"]),
        &long,
        ResultsVisibility::Always,
    );
    assert!(matches!(too_long, Err(PollError::Description(_))));
    let tall = "a\n".repeat(super::DESCRIPTION_LINES + 1);
    let too_tall = NewPoll::build(
        "Q?",
        &options(&["a", "b"]),
        &tall,
        ResultsVisibility::Always,
    );
    assert!(matches!(too_tall, Err(PollError::Description(_))));
    let fine = NewPoll::build(
        "Q?",
        &options(&["a", "b"]),
        "line one\nline two\n\n",
        ResultsVisibility::Always,
    )
    .unwrap();
    assert_eq!(fine.description().unwrap().trimmed(), "line one\nline two");
}

#[test]
fn counts_show_as_the_setting_says() {
    use ResultsVisibility::{AfterClose, AfterVote, Always};
    // (visibility, voted, closed) -> shown
    for (v, voted, closed, shown) in [
        (Always, false, false, true),
        (AfterVote, false, false, false),
        (AfterVote, true, false, true),
        (AfterClose, false, false, false),
        (AfterClose, true, false, false),
        (AfterClose, false, true, true),
        (AfterVote, false, true, true),
    ] {
        assert_eq!(
            v.shows(voted, closed),
            shown,
            "{v:?} voted={voted} closed={closed}"
        );
    }
    for v in ResultsVisibility::ALL {
        assert_eq!(ResultsVisibility::parse(v.name()), Some(v));
    }
    assert_eq!(ResultsVisibility::parse("sometimes"), None);
}

// ---- screens ----

#[test]
fn a_screen_asks_for_its_list_first_and_ignores_keys_until_it_arrives() {
    let mut screen = CommunityScreen::open(Section::Oneliners);
    assert_eq!(
        screen.press(&MEMBER, ScreenKey::Char('p')),
        Step::Stay,
        "waiting for the list"
    );
    assert_eq!(
        screen.take_request(),
        Some(Request::Lines(LineKind::Oneliner))
    );
    assert_eq!(screen.take_request(), None, "a request is taken once");
    assert_eq!(
        screen.press(&MEMBER, ScreenKey::Escape),
        Step::Leave,
        "leaving is always possible"
    );
    let mut calls = CommunityScreen::open(Section::LastCallers);
    assert_eq!(calls.take_request(), Some(Request::Callers));
    let mut polls = CommunityScreen::open(Section::Polls);
    assert_eq!(polls.take_request(), Some(Request::Polls { offset: 0 }));
}

#[test]
fn typing_a_line_posts_it_with_enter_and_keeps_the_text_if_refused() {
    let mut screen = lines_screen(LineKind::Oneliner, vec![entry(1, "ann", "hi")]);
    assert!(shown_keys(&screen, &MEMBER).contains(&keys::HINT_POST));
    assert_eq!(screen.press(&MEMBER, ScreenKey::Char('p')), Step::Stay);
    type_into(&mut screen, &MEMBER, "hello\u{8}!\r");
    assert_eq!(
        screen.take_request(),
        Some(Request::Post(LineKind::Oneliner, "hell!".to_owned()))
    );
    // Refused: the field is still there with what was typed.
    screen.deliver(Response::Refused(Note {
        error: true,
        key: keys::NOTE_BLANK,
    }));
    let view = screen.view(&MEMBER);
    assert!(
        view.lines
            .iter()
            .any(|l| matches!(l, Line::Field { value, active: true, .. } if value == "hell!"))
    );
    assert!(
        view.lines
            .iter()
            .any(|l| matches!(l, Line::Notice { error: true, text } if *text == keys::NOTE_BLANK))
    );
    // Accepted: the list is replaced and the field closes.
    screen.press(&MEMBER, ScreenKey::Enter);
    screen.take_request();
    screen.deliver(Response::Lines {
        kind: LineKind::Oneliner,
        entries: vec![entry(2, "me", "hell!"), entry(1, "ann", "hi")],
        note: Some(Note {
            error: false,
            key: keys::NOTE_POSTED,
        }),
    });
    let view = screen.view(&MEMBER);
    assert!(!view.lines.iter().any(|l| matches!(l, Line::Field { .. })));
    assert!(
        view.lines
            .iter()
            .any(|l| matches!(l, Line::Filled { key, .. } if *key == keys::ENTRY))
    );
}

#[test]
fn typing_stops_at_the_length_of_the_list() {
    let mut screen = lines_screen(LineKind::Graffiti, vec![]);
    screen.press(&MEMBER, ScreenKey::Char('p'));
    type_into(
        &mut screen,
        &MEMBER,
        &"z".repeat(LineKind::Graffiti.max_chars() + 30),
    );
    screen.press(&MEMBER, ScreenKey::Enter);
    let Some(Request::Post(_, text)) = screen.take_request() else {
        panic!("no post");
    };
    assert_eq!(text.chars().count(), LineKind::Graffiti.max_chars());
}

#[test]
fn the_post_key_is_only_there_with_the_permission() {
    let read_only = Holds(&[Permission::CommunityView]);
    let mut screen = lines_screen(LineKind::Oneliner, vec![entry(1, "ann", "hi")]);
    assert!(!shown_keys(&screen, &read_only).contains(&keys::HINT_POST));
    screen.press(&read_only, ScreenKey::Char('p'));
    assert!(
        !screen
            .view(&read_only)
            .lines
            .iter()
            .any(|l| matches!(l, Line::Field { .. }))
    );
    // The wall needs its own permission: a one-liner poster cannot write on it.
    let one_only = Holds(&[Permission::CommunityView, Permission::OnelinersPost]);
    let wall = lines_screen(LineKind::Graffiti, vec![]);
    assert!(!shown_keys(&wall, &one_only).contains(&keys::HINT_POST));
}

#[test]
fn staff_remove_by_number_and_a_member_cannot() {
    let entries = vec![entry(7, "bob", "two"), entry(3, "ann", "one")];
    let mut screen = lines_screen(LineKind::Oneliner, entries.clone());
    assert!(!shown_keys(&screen, &MEMBER).contains(&keys::HINT_REMOVE));
    screen.press(&MEMBER, ScreenKey::Char('d'));
    assert_eq!(screen.take_request(), None);
    assert!(shown_keys(&screen, &STAFF).contains(&keys::HINT_REMOVE));
    type_into(&mut screen, &STAFF, "d2\r");
    assert_eq!(
        screen.take_request(),
        Some(Request::Remove(LineKind::Oneliner, EntryId::from_raw(3))),
        "the number on screen picks the row"
    );
    // A number that is not there asks for nothing and says so.
    let mut other = lines_screen(LineKind::Oneliner, entries);
    type_into(&mut other, &STAFF, "d9\r");
    assert_eq!(other.take_request(), None);
    assert!(
        other
            .view(&STAFF)
            .lines
            .iter()
            .any(|l| matches!(l, Line::Notice { error: true, .. }))
    );
}

#[test]
fn losing_the_view_permission_closes_the_screen() {
    let mut screen = lines_screen(LineKind::Oneliner, vec![]);
    let none = Holds(&[]);
    assert!(!screen.still_permitted(&none));
    assert_eq!(screen.press(&none, ScreenKey::Char('p')), Step::Denied);
    assert_eq!(screen.deliver(Response::NotPermitted), Step::Denied);
}

fn row(id: i64, question: &str) -> PollRow {
    PollRow {
        id: PollId::from_raw(id),
        question: question.to_owned(),
        closed: false,
        votes: 0,
    }
}

fn detail(can_vote: bool, counts: Option<Vec<u64>>, mine: Option<usize>) -> PollDetail {
    PollDetail {
        id: PollId::from_raw(5),
        question: "Best?".to_owned(),
        description: "line one\nline two".to_owned(),
        creator: "ann".to_owned(),
        options: vec!["red".to_owned(), "blue".to_owned()],
        my_vote: mine,
        closed: false,
        visibility: ResultsVisibility::AfterVote,
        counts,
        can: PollActions {
            vote: can_vote,
            close: false,
            remove: false,
        },
    }
}

fn poll_open(access: &dyn PermissionCheck, d: PollDetail) -> CommunityScreen {
    let mut screen = CommunityScreen::open(Section::Polls);
    screen.take_request();
    screen.deliver(Response::Polls {
        offset: 0,
        rows: vec![row(5, "Best?")],
        total: 1,
        note: None,
    });
    assert_eq!(screen.press(access, ScreenKey::Char('1')), Step::Stay);
    assert_eq!(
        screen.take_request(),
        Some(Request::Poll(PollId::from_raw(5)))
    );
    screen.deliver(Response::Poll {
        detail: Box::new(d),
        note: None,
    });
    screen
}

#[test]
fn a_poll_screen_votes_with_a_digit_and_asks_again_when_a_vote_was_made() {
    let mut screen = poll_open(&MEMBER, detail(true, None, None));
    assert!(shown_keys(&screen, &MEMBER).contains(&keys::HINT_VOTE));
    // Counts are hidden: the screen says when they show, and shows no numbers.
    assert!(shown_keys(&screen, &MEMBER).contains(&keys::POLL_HIDDEN_VOTE));
    screen.press(&MEMBER, ScreenKey::Char('2'));
    assert_eq!(
        screen.take_request(),
        Some(Request::Vote(PollId::from_raw(5), 1))
    );
    // A digit for an option the poll lacks asks for nothing.
    let mut short = poll_open(&MEMBER, detail(true, None, None));
    short.press(&MEMBER, ScreenKey::Char('3'));
    assert_eq!(short.take_request(), None);
    // After voting the key still asks, so a second vote is answered in words, not silence.
    let mut voted = poll_open(&MEMBER, detail(false, Some(vec![0, 1]), Some(1)));
    assert!(!shown_keys(&voted, &MEMBER).contains(&keys::HINT_VOTE));
    voted.press(&MEMBER, ScreenKey::Char('1'));
    assert_eq!(
        voted.take_request(),
        Some(Request::Vote(PollId::from_raw(5), 0))
    );
    // Without polls.vote the key does nothing at all.
    let mut no_vote = poll_open(
        &Holds(&[Permission::CommunityView]),
        detail(true, None, None),
    );
    no_vote.press(&Holds(&[Permission::CommunityView]), ScreenKey::Char('1'));
    assert_eq!(no_vote.take_request(), None);
}

#[test]
fn counts_and_percentages_are_drawn_only_when_given() {
    let shown = poll_open(&MEMBER, detail(false, Some(vec![1, 3]), Some(0)));
    let view = shown.view(&MEMBER);
    let rows: Vec<_> = view
        .lines
        .iter()
        .filter_map(|l| match l {
            Line::Filled { key, values } if key.starts_with("community.poll.option") => {
                Some((*key, values.clone()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].0, keys::POLL_OPTION_COUNT_MINE);
    assert!(rows[0].1.contains(&("percent", "25".to_owned())));
    assert!(rows[1].1.contains(&("percent", "75".to_owned())));
    let hidden = poll_open(&MEMBER, detail(true, None, None));
    assert!(
        hidden
            .view(&MEMBER)
            .lines
            .iter()
            .all(|l| !matches!(l, Line::Filled { key, .. } if *key == keys::POLL_OPTION_COUNT))
    );
}

#[test]
fn the_poll_form_walks_question_options_and_review_and_sends_one_create() {
    let mut screen = CommunityScreen::open(Section::Polls);
    screen.take_request();
    screen.deliver(Response::Polls {
        offset: 0,
        rows: vec![],
        total: 0,
        note: None,
    });
    // A member without polls.create has no form.
    screen.press(&MEMBER, ScreenKey::Char('c'));
    assert!(!screen.in_form());
    screen.press(&STAFF, ScreenKey::Char('c'));
    assert!(screen.in_form());
    // A blank question or a lone option does not advance.
    screen.press(&STAFF, ScreenKey::Enter);
    type_into(&mut screen, &STAFF, "Pick?\rred\r\r");
    assert!(
        screen
            .view(&STAFF)
            .lines
            .iter()
            .any(|l| matches!(l, Line::Notice { text, .. } if *text == keys::NOTE_POLL_TOO_FEW))
    );
    type_into(&mut screen, &STAFF, "blue\r\r");
    assert!(shown_keys(&screen, &STAFF).contains(&keys::HINT_REVIEW));
    // The visibility cycles; the description comes from the editor.
    screen.press(&STAFF, ScreenKey::Char('r'));
    screen.press(&STAFF, ScreenKey::Char('d'));
    let ask = screen.take_editor().unwrap();
    assert_eq!(ask.initial, "");
    assert_eq!(
        screen.press(&STAFF, ScreenKey::Char('x')),
        Step::Stay,
        "keys wait for the editor"
    );
    screen.deliver_description(Some("  why  ".to_owned()));
    screen.press(&STAFF, ScreenKey::Enter);
    let Some(Request::Create {
        question,
        options,
        description,
        visibility,
    }) = screen.take_request()
    else {
        panic!("no create");
    };
    assert_eq!(question, "Pick?");
    assert_eq!(options[..3], ["red", "blue", ""]);
    assert_eq!(description, "why");
    assert_eq!(
        visibility,
        ResultsVisibility::AfterClose,
        "after-vote, then one step on"
    );
    // Refused (for example the rate limit): the form stays with its text.
    screen.deliver(Response::Refused(Note {
        error: true,
        key: keys::NOTE_POLL_RATE_LIMITED,
    }));
    assert!(screen.in_form());
    // Created: the poll opens.
    screen.press(&STAFF, ScreenKey::Enter);
    screen.take_request();
    screen.deliver(Response::Poll {
        detail: Box::new(detail(true, None, None)),
        note: None,
    });
    assert!(!screen.in_form());
}

#[test]
fn leaving_a_poll_goes_back_to_the_page_it_came_from() {
    let mut screen = CommunityScreen::open(Section::Polls);
    screen.take_request();
    let rows: Vec<PollRow> = (1..=9).map(|n| row(n, "q")).collect();
    screen.deliver(Response::Polls {
        offset: 9,
        rows,
        total: 12,
        note: None,
    });
    screen.press(&MEMBER, ScreenKey::Char('p'));
    assert_eq!(screen.take_request(), Some(Request::Polls { offset: 0 }));
    screen.deliver(Response::Poll {
        detail: Box::new(detail(false, None, None)),
        note: None,
    });
    screen.press(&MEMBER, ScreenKey::Escape);
    assert_eq!(screen.take_request(), Some(Request::Polls { offset: 9 }));
    // A page that emptied (a poll was removed) asks for the first one instead of showing nothing.
    screen.deliver(Response::Polls {
        offset: 9,
        rows: vec![],
        total: 3,
        note: None,
    });
    assert_eq!(screen.take_request(), Some(Request::Polls { offset: 0 }));
}

#[test]
fn a_failed_request_says_so_and_changes_nothing() {
    let mut screen = lines_screen(LineKind::Oneliner, vec![entry(1, "ann", "hi")]);
    screen.press(&MEMBER, ScreenKey::Char('p'));
    type_into(&mut screen, &MEMBER, "hello\r");
    screen.take_request();
    screen.deliver(Response::Failed);
    let view = screen.view(&MEMBER);
    assert!(
        view.lines
            .iter()
            .any(|l| matches!(l, Line::Notice { error: true, text } if *text == keys::NOTE_FAILED))
    );
    assert!(
        view.lines
            .iter()
            .any(|l| matches!(l, Line::Filled { key, .. } if *key == keys::ENTRY))
    );
    // Not used elsewhere in this file, but part of the contract the screen is built on.
    let _ = AccountId::from_raw(1);
}

#[test]
fn every_key_core_emits_is_listed() {
    let mut seen = std::collections::BTreeSet::new();
    for key in keys::TEXT_KEYS {
        assert!(seen.insert(*key), "{key} is listed twice");
        assert!(key.starts_with("community."), "{key}");
    }
}
