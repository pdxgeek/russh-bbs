//! The social layer: structured schemes one pupil plays on another, template rumours, the school
//! election, the awards and the opt-out. The rules are tested on the engine with a shared world
//! (`School`), and the two-player flow once more through the framework's real store.
#![allow(
    clippy::cast_precision_loss,
    clippy::needless_pass_by_value,
    clippy::doc_markdown,
    clippy::similar_names,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::type_complexity,
    clippy::collapsible_if,
    clippy::cast_possible_wrap,
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::print_stderr,
    clippy::panic
)]
mod common;

use std::sync::Arc;

use common::{content, monday};
use russh_bbs_core::doors::{DoorStore, Revision, SaveWrite};
use russh_bbs_core::limits::{Clock, Day, ManualClock};
use russh_bbs_core::store::AccountId;
use russh_bbs_door::host::Running;
use russh_bbs_door::sim::MemoryDoorStore;
use russh_bbs_door::{Door, DoorEnv, DoorKey, DoorRng};
use russh_bbs_door_hallpass::commons::{PLOT_PER_TARGET, plot_state};
use russh_bbs_door_hallpass::content::Content;
use russh_bbs_door_hallpass::game::{Action, Event, Game};
use russh_bbs_door_hallpass::save::{decode_player, decode_world, encode_player};
use russh_bbs_door_hallpass::state::{Player, World, map_set};
use russh_bbs_door_hallpass::world::{WorldDelta, tick_day};
use russh_bbs_door_hallpass::{Hallpass, SAVE_VERSION, WORLD_VERSION};

/// A school several pupils share. A pupil's session reads the world when it opens and sends its
/// changes back, exactly as the board does.
struct School {
    world: World,
    content: Arc<Content>,
    rng: DoorRng,
}

impl School {
    fn new(day: i64) -> Self {
        Self {
            world: World::new(day - 12),
            content: content(),
            rng: DoorRng::from_seed(5),
        }
    }

    /// A new pupil enrols on `day` and is on the roster.
    fn enrol(&mut self, name: &str, trait_id: &str, day: i64) -> Game {
        let (mut g, _) = Game::open(&self.content, name, None, Some(&self.world.to_bytes()), day);
        g.apply(&Action::Enrol(trait_id.to_owned()), &mut self.rng)
            .unwrap();
        g.player.nerve = 10;
        g.player.nerve_max = 10;
        self.commit(&mut g);
        g
    }

    /// A pupil comes back on `day` (the world is read as it is now).
    fn reopen(&mut self, player: &Player, day: i64) -> (Game, Vec<Event>) {
        let (mut g, events) = Game::open(
            &self.content,
            &player.name,
            Some(player.clone()),
            Some(&self.world.to_bytes()),
            day,
        );
        self.commit(&mut g);
        (g, events)
    }

    /// Send what a session asked for to the world.
    fn commit(&mut self, g: &mut Game) {
        for d in g.take_world_changes() {
            if let Some(bytes) =
                d.apply_to_bytes(Some(&self.world.to_bytes()), g.player.day, &self.content)
            {
                self.world = World::from_bytes(Some(&bytes), 0);
            }
        }
    }

    fn act(&mut self, g: &mut Game, action: Action) -> Vec<Event> {
        let events = g
            .apply(&action, &mut self.rng)
            .unwrap_or_else(|e| panic!("{action:?} refused: {e}"));
        self.commit(g);
        events
    }

    /// Run the world's daily tick up to and including `day`.
    fn tick_to(&mut self, day: i64) {
        for d in (self.world.last_day + 1)..=day {
            tick_day(
                &mut self.world,
                &self.content,
                d,
                &mut DoorRng::from_seed(1),
            );
        }
    }
}

fn said(events: &[Event]) -> String {
    events
        .iter()
        .map(Event::describe)
        .collect::<Vec<_>>()
        .join(" | ")
}

fn plot(kind: &str, to: &str) -> Action {
    Action::Plot {
        kind: kind.to_owned(),
        to: to.to_owned(),
    }
}

/// A pupil who can play a swapped timetable (a forgery kit) and has nerve to spare.
fn forger(school: &mut School, name: &str, day: i64) -> Game {
    let mut g = school.enrol(name, "smooth_talker", day);
    map_set(&mut g.player.inventory, "forgery_kit", 1);
    g
}

// ---------------------------------------------------------------- plots

#[test]
fn a_scheme_on_another_pupil_is_posted_delivered_and_its_outcome_comes_back_to_the_sender() {
    let day = monday() + 3;
    let mut school = School::new(day);
    let mut bea = school.enrol("bea", "fast_runner", day);
    let mut ann = forger(&mut school, "ann", day);
    // The roster knows both, and ann can see bea (not herself).
    assert_eq!(
        ann.classmates()
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        ["bea"]
    );
    ann.world = school.world.clone();
    let nerve = ann.player.nerve;
    let events = school.act(&mut ann, plot("swapped_timetable", "bea"));
    assert!(
        said(&events).contains("in motion for bea"),
        "{}",
        said(&events)
    );
    assert_eq!(
        i32::from(ann.player.nerve),
        i32::from(nerve) - 2,
        "it costs nerve"
    );
    let posted = &school.world.commons.plots;
    assert_eq!(posted.len(), 1);
    assert_eq!(
        (
            posted[0].from.as_str(),
            posted[0].to.as_str(),
            posted[0].state
        ),
        ("ann", "bea", plot_state::WAITING)
    );

    // Bea comes back the next morning: it lands (no counterplay), costs her a little, and
    // protects her for two days.
    let (cool, nerve_max) = (bea.player.cool, bea.player.nerve_max);
    let (bea2, events) = school.reopen(&bea.player, day + 1);
    let text = said(&events);
    assert!(text.contains("swapped") && text.contains("ann"), "{text}");
    assert_eq!(bea2.player.cool, cool - 1);
    assert_eq!(
        bea2.player.nerve,
        bea2.player.nerve_max.saturating_sub(1).min(nerve_max)
    );
    assert_eq!(bea2.player.social.protected_until, day + 1 + 2);
    assert!(
        !bea2.player.social.inbox.is_empty(),
        "the school remembers what was done to her"
    );
    assert_eq!(school.world.commons.plots[0].state, plot_state::LANDED);
    bea = bea2;

    // Ann comes back and hears how it went; the record leaves the post.
    let legend = ann.player.legend;
    let (ann2, events) = school.reopen(&ann.player, day + 1);
    assert!(said(&events).contains("it landed"), "{}", said(&events));
    assert_eq!(ann2.player.legend, legend + 1);
    assert!(school.world.commons.plots.is_empty());
    // Hearing it twice is impossible: the same visit again has nothing.
    let (_, again) = school.reopen(&ann2.player, day + 1);
    assert!(!said(&again).contains("landed"));
    let _ = bea;
}

#[test]
fn a_build_or_a_kit_is_the_counterplay_and_a_seen_through_scheme_costs_the_sender() {
    let day = monday() + 3;
    for (counter, how) in [
        ("item:spare_card", "a spare card"),
        ("trait:teachers_pet", "a teacher's pet (trusted)"),
    ] {
        let mut school = School::new(day);
        let trait_id = if counter.starts_with("trait") {
            "teachers_pet"
        } else {
            "fast_runner"
        };
        let mut bea = school.enrol("bea", trait_id, day);
        if counter.starts_with("item") {
            map_set(&mut bea.player.inventory, "spare_card", 1);
        }
        let mut ann = forger(&mut school, "ann", day);
        ann.world = school.world.clone();
        school.act(&mut ann, plot("swapped_timetable", "bea"));
        let cool = bea.player.cool;
        let (bea2, events) = school.reopen(&bea.player, day + 1);
        assert!(
            said(&events).contains("saw it") || said(&events).contains("spare card"),
            "{how}: {}",
            said(&events)
        );
        assert_eq!(bea2.player.cool, cool, "{how}: nothing landed");
        assert_eq!(school.world.commons.plots[0].state, plot_state::COUNTERED);
        let before = ann.player.cool;
        let (ann2, events) = school.reopen(&ann.player, day + 1);
        assert!(said(&events).contains("saw it coming"));
        assert_eq!(
            ann2.player.cool,
            before - 1,
            "a seen-through scheme embarrasses the sender"
        );
    }
    // A pupil with neither is hit; the same scheme ends differently for different builds.
    let mut school = School::new(day);
    let bea = school.enrol("bea", "fast_runner", day);
    let mut ann = forger(&mut school, "ann", day);
    ann.world = school.world.clone();
    school.act(&mut ann, plot("swapped_timetable", "bea"));
    let _ = school.reopen(&bea.player, day + 1);
    assert_eq!(school.world.commons.plots[0].state, plot_state::LANDED);
}

#[test]
fn what_a_scheme_needs_depends_on_the_sender_and_the_list_never_offers_what_it_refuses() {
    let day = monday() + 3;
    let mut school = School::new(day);
    let _bea = school.enrol("bea", "fast_runner", day);
    let mut plain = school.enrol("cal", "fast_runner", day);
    plain.world = school.world.clone();
    let offered = |g: &Game| -> Vec<String> {
        g.legal_actions()
            .into_iter()
            .filter_map(|a| match a {
                Action::Plot { kind, to } if to == "bea" => Some(kind),
                _ => None,
            })
            .collect()
    };
    // A runner with nothing in his pockets has the kind plots and the laces, not the forgeries.
    let kinds = offered(&plain);
    assert!(kinds.contains(&"squeaky_laces".to_owned()) && kinds.contains(&"pep_talk".to_owned()));
    assert!(
        !kinds.contains(&"swapped_timetable".to_owned()),
        "{kinds:?}"
    );
    assert!(!kinds.contains(&"glued_locker".to_owned()));
    // A forger gains the swap; a tinkerer the glue.
    let mut ann = forger(&mut school, "ann", day);
    ann.world = school.world.clone();
    assert!(offered(&ann).contains(&"swapped_timetable".to_owned()));
    let mut pip = school.enrol("pip", "tinkerer", day);
    pip.world = school.world.clone();
    assert!(offered(&pip).contains(&"glued_locker".to_owned()));
    // Every action in the list is accepted by the engine (the list is built by trying them).
    for a in pip.legal_actions() {
        assert!(pip.can(&a), "{a:?}");
    }
}

#[test]
fn nobody_is_piled_on_and_a_target_is_left_alone_after_one_reaches_them() {
    let day = monday() + 3;
    let mut school = School::new(day);
    let bea = school.enrol("bea", "fast_runner", day);
    let mut senders: Vec<Game> = (0..4)
        .map(|i| forger(&mut school, &format!("s{i}"), day))
        .collect();
    for s in &mut senders {
        s.world = school.world.clone();
    }
    // The same sender may not go at the same pupil twice within the rest days.
    school.act(&mut senders[0], plot("swapped_timetable", "bea"));
    senders[0].player.nerve = 10;
    senders[0].world = school.world.clone();
    let why = senders[0].plot_block("swapped_timetable", "bea").unwrap();
    assert!(why.contains("give it a few days"), "{why}");
    // Two senders more fill bea's post to the per-target cap; the next is refused.
    for sender in senders.iter_mut().take(PLOT_PER_TARGET).skip(1) {
        sender.world = school.world.clone();
        school.act(sender, plot("swapped_timetable", "bea"));
    }
    senders[PLOT_PER_TARGET].world = school.world.clone();
    let why = senders[PLOT_PER_TARGET]
        .plot_block("swapped_timetable", "bea")
        .unwrap();
    assert!(why.contains("post is full"), "{why}");
    assert!(
        senders[PLOT_PER_TARGET]
            .apply(&plot("swapped_timetable", "bea"), &mut school.rng)
            .is_err()
    );
    assert_eq!(school.world.commons.waiting_for("bea"), PLOT_PER_TARGET);

    // Bea opens: the first lands and protects her; the other two are turned away.
    let (_, events) = school.reopen(&bea.player, day + 1);
    assert_eq!(
        events
            .iter()
            .filter(|e| e.describe().contains("swapped"))
            .count(),
        1,
        "{}",
        said(&events)
    );
    let states: Vec<u8> = school.world.commons.plots.iter().map(|p| p.state).collect();
    let count = |want: u8| {
        school
            .world
            .commons
            .plots
            .iter()
            .filter(|p| p.state == want)
            .count()
    };
    assert_eq!(count(plot_state::LANDED), 1, "{states:?}");
    assert_eq!(count(plot_state::TURNED), PLOT_PER_TARGET - 1);
    // The senders of the turned-away ones are told it never reached them, at no cost.
    let (_, events) = school.reopen(&senders[1].player, day + 1);
    assert!(said(&events).contains("never reached"), "{}", said(&events));
}

#[test]
fn a_sender_may_play_only_so_many_a_day() {
    let day = monday() + 3;
    let mut school = School::new(day);
    let names = ["b1", "b2", "b3"];
    for n in names {
        school.enrol(n, "fast_runner", day);
    }
    let mut ann = forger(&mut school, "ann", day);
    ann.world = school.world.clone();
    ann.player.slot = 3;
    school.act(&mut ann, plot("squeaky_laces", "b1"));
    ann.world = school.world.clone();
    school.act(&mut ann, plot("squeaky_laces", "b2"));
    ann.world = school.world.clone();
    let why = ann.plot_block("squeaky_laces", "b3").unwrap();
    assert!(why.contains("enough scheming"), "{why}");
    // And the world holds the same line even if a session had not noticed.
    let mut world = school.world.clone();
    WorldDelta::Plot {
        from: "ann".into(),
        to: "b3".into(),
        kind: "squeaky_laces".into(),
        day,
    }
    .apply(&mut world, &school.content);
    assert_eq!(
        world.commons.plots.len(),
        2,
        "the stored world refuses the third"
    );
}

#[test]
fn kind_schemes_land_without_a_counterplay_and_do_not_protect() {
    let day = monday() + 3;
    let mut school = School::new(day);
    let bea = school.enrol("bea", "fast_runner", day);
    let mut ann = school.enrol("ann", "smooth_talker", day);
    ann.player.cool = 8;
    ann.world = school.world.clone();
    school.act(&mut ann, plot("pep_talk", "bea"));
    let (bea2, events) = school.reopen(&bea.player, day + 1);
    assert!(said(&events).contains("exactly what you needed"));
    assert_eq!(
        bea2.player.social.protected_until, 0,
        "a favour is not a reason to be left alone"
    );
    assert!(bea2.player.nerve <= bea2.player.nerve_max);
}

// ---------------------------------------------------------------- the opt-out

#[test]
fn an_opted_out_pupil_cannot_be_targeted_and_cannot_target() {
    let day = monday() + 3;
    let mut school = School::new(day);
    let mut bea = school.enrol("bea", "fast_runner", day);
    let mut ann = forger(&mut school, "ann", day);
    // Bea opts out; the roster says so.
    let events = school.act(&mut bea, Action::OptOut(true));
    assert!(said(&events).contains("opted out"));
    assert!(school.world.commons.opted_out("bea"));
    // Ann's menu lists bea as unreachable and her engine refuses.
    ann.world = school.world.clone();
    assert!(ann.classmates()[0].opted_out);
    assert!(
        ann.plot_block("squeaky_laces", "bea")
            .unwrap()
            .contains("opted out")
    );
    assert!(
        ann.legal_actions()
            .iter()
            .all(|a| !matches!(a, Action::Plot { to, .. } if to == "bea"))
    );
    assert!(
        ann.apply(&plot("squeaky_laces", "bea"), &mut school.rng)
            .is_err()
    );
    // An opted-out pupil cannot play one either, nor send a kind word.
    bea.world = school.world.clone();
    let why = bea.plot_block("pep_talk", "ann").unwrap();
    assert!(why.contains("opted out"), "{why}");
    // The stored world is the authority: a session working from a stale snapshot that still
    // thought bea was open cannot put a plot in the post.
    let mut stale = ann.clone();
    let open_world = {
        let mut w = school.world.clone();
        w.commons.roster.iter_mut().for_each(|p| p.opt_out = false);
        w
    };
    stale.world = open_world;
    assert!(
        stale.plot_block("squeaky_laces", "bea").is_none(),
        "the stale view allows it"
    );
    stale
        .apply(&plot("squeaky_laces", "bea"), &mut school.rng)
        .unwrap();
    school.commit(&mut stale);
    assert!(
        school.world.commons.plots.is_empty(),
        "but the stored world turns it away: {:?}",
        school.world.commons.plots
    );
    // A plot posted before the pupil opted out is turned away at delivery, with no effect.
    let day2 = day + 1;
    let mut cal = school.enrol("cal", "fast_runner", day2);
    cal.world = school.world.clone();
    let mut dee = forger(&mut school, "dee", day2);
    dee.world = school.world.clone();
    school.act(&mut dee, plot("swapped_timetable", "cal"));
    let cool = cal.player.cool;
    school.act(&mut cal, Action::OptOut(true));
    let (cal2, events) = school.reopen(&cal.player, day2 + 1);
    assert_eq!(cal2.player.cool, cool, "nothing landed: {}", said(&events));
    assert_eq!(school.world.commons.plots[0].state, plot_state::TURNED);
    // Opting back in reopens the door both ways.
    let mut cal3 = cal2;
    school.act(&mut cal3, Action::OptOut(false));
    assert!(!school.world.commons.opted_out("cal"));
}

#[test]
fn the_opt_out_survives_the_term_and_does_not_stop_rumours_the_election_or_awards() {
    let day = monday() + 3;
    let mut school = School::new(day);
    let mut bea = school.enrol("bea", "fast_runner", day);
    school.act(&mut bea, Action::OptOut(true));
    // Rumours and the election are not aimed at a player: they stay open.
    bea.world = school.world.clone();
    assert!(
        bea.legal_actions()
            .iter()
            .any(|a| matches!(a, Action::Spread { .. }))
            || bea
                .rumour_block("fair_play", "tilly")
                .is_some_and(|w| w.contains("nobody")),
        "only the lack of an audience stops a rumour"
    );
    // A new term: the character is new, the choice is not.
    let term_days = school.content.rules.term_days;
    school.tick_to(school.world.term_start + term_days);
    assert_eq!(school.world.term, 2);
    let (bea2, _) = school.reopen(&bea.player, school.world.term_start + 1);
    assert_eq!(bea2.player.term, 2);
    assert!(bea2.player.social.opt_out, "the opt-out follows the pupil");
}

// ---------------------------------------------------------------- rumours

#[test]
fn a_rumour_is_chosen_from_a_template_and_changes_the_school_and_the_speaker() {
    let day = monday() + 3;
    let mut school = School::new(day);
    let mut ann = school.enrol("ann", "smooth_talker", day);
    // Somewhere with an audience: the common room at lunch, where students sit.
    let c = content();
    let who = c
        .npcs
        .iter()
        .find(|n| !n.is_authority())
        .map(|n| n.id.clone())
        .unwrap();
    let mut found = None;
    'search: for slot in 0..9 {
        for place in &c.places {
            ann.player.slot = slot;
            ann.player.place = place.id.clone();
            if ann.rumour_block("fair_play", &who).is_none() {
                found = Some((slot, place.id.clone()));
                break 'search;
            }
        }
    }
    let (slot, place) = found.expect("somewhere has an audience");
    ann.player.slot = slot;
    ann.player.place = place;
    let bond = ann.bond(&who);
    let events = school.act(
        &mut ann,
        Action::Spread {
            rumour: "fair_play".into(),
            about: who.clone(),
        },
    );
    assert!(said(&events).contains("fairest"), "{}", said(&events));
    assert_eq!(ann.bond(&who), bond + 1, "praise wins a friend");
    assert_eq!(school.world.commons.rumours.len(), 1);
    assert_eq!(school.world.commons.rumours[0].by, "ann");
    // Everyone's paper lists it, until it stops going round.
    let (bea, _) = Game::open(&c, "bea", None, Some(&school.world.to_bytes()), day);
    assert!(bea.rumour_lines()[0].contains("fairest"));
    // One a day: the second is refused with the reason.
    ann.world = school.world.clone();
    assert!(
        ann.rumour_block("fair_play", &who)
            .unwrap()
            .contains("had your say")
    );
    // A week later nobody repeats it.
    school.tick_to(day + c.social.rules.rumour_days + 1);
    assert!(school.world.commons.rumours.is_empty());
}

#[test]
fn rumours_can_only_praise_a_pupil_or_gossip_about_authority_never_a_stray_or_a_pupil() {
    let c = content();
    for r in &c.social.rumours {
        let subjects = c.rumour_subjects(r);
        assert!(!subjects.is_empty(), "{}", r.id);
        for s in &subjects {
            let npc = c.npc(s);
            if r.kind == "praise" {
                continue;
            }
            assert_ne!(s, "strays", "{} can slander the Strays", r.id);
            if let Some(n) = npc {
                assert!(n.is_authority() && n.prankable, "{} names {}", r.id, n.id);
                assert_ne!(n.clique.as_deref(), Some("strays"));
            }
        }
        // A rumour never names a player: the subject is a character, a clique or the speaker.
        assert!(["npc", "clique", "self"].contains(&r.about.as_str()));
    }
    // A subject outside the template's list is refused by the engine.
    let day = monday() + 3;
    let mut school = School::new(day);
    let mut ann = school.enrol("ann", "smooth_talker", day);
    for slot in 0..9 {
        ann.player.slot = slot;
        if ann.rumour_block("biscuits", "tilly").is_some() {
            continue;
        }
        panic!("a rumour about a Stray was allowed");
    }
}

// ---------------------------------------------------------------- the election

fn campaign(school: &mut School, g: &mut Game, days: i64) {
    for d in 0..days {
        g.sync_day(g.player.day + i64::from(d > 0));
        g.player.nerve = g.player.nerve_max;
        g.world = school.world.clone();
        for _ in 0..2 {
            if g.can(&Action::Canvass) {
                school.act(g, Action::Canvass);
            }
        }
    }
}

#[test]
fn an_election_is_stood_in_canvassed_voted_and_counted_and_the_winner_changes_the_school() {
    let day = monday() + 3; // the world began 12 days ago: the election is open
    let mut school = School::new(day);
    let mut ann = school.enrol("ann", "smooth_talker", day);
    let mut bea = school.enrol("bea", "fast_runner", day);
    ann.player.cool = 40;
    ann.player.add_standing("lamplighters", 40);
    // Not before the nominations open.
    let mut early = school.enrol("early", "smooth_talker", day);
    early.world.term_start = early.player.day - 3;
    assert!(early.apply(&Action::Nominate, &mut school.rng).is_err());
    // A poor record cannot stand.
    let mut rogue = school.enrol("rogue", "smooth_talker", day);
    rogue
        .player
        .file
        .push(russh_bbs_door_hallpass::state::FileEntry {
            day,
            kind: "caught".into(),
            sev: 8,
        });
    assert!(rogue.apply(&Action::Nominate, &mut school.rng).is_err());
    school.act(&mut ann, Action::Nominate);
    assert!(
        ann.apply(&Action::Nominate, &mut school.rng).is_err(),
        "only once"
    );
    assert_eq!(school.world.commons.election.candidates.len(), 1);
    campaign(&mut school, &mut ann, 3);
    let backing = school.world.commons.backing("ann");
    assert!(backing >= 8, "canvassing works: {backing}");
    assert!(
        backing < russh_bbs_door_hallpass::commons::BACKING_CAP,
        "and a few days do not win a school"
    );
    // Each door knocked on wins less than the last as the campaign saturates, and never past the cap.
    let first = {
        let (mut fresh, _) =
            Game::open(&content(), "z", None, Some(&World::new(0).to_bytes()), day);
        fresh.world.term_start = day - 12;
        fresh
            .apply(&Action::Enrol("smooth_talker".into()), &mut school.rng)
            .unwrap();
        fresh.player.cool = 40;
        fresh.apply(&Action::Nominate, &mut school.rng).unwrap();
        fresh.apply(&Action::Canvass, &mut school.rng).unwrap();
        fresh.world.commons.backing("z")
    };
    assert!(first >= 3, "a first door wins a few: {first}");
    // A fortnight of it saturates at the cap.
    for _ in 0..30 {
        WorldDelta::Backing {
            who: "ann".into(),
            add: 4,
        }
        .apply(&mut school.world, &school.content);
    }
    assert_eq!(
        school.world.commons.backing("ann"),
        russh_bbs_door_hallpass::commons::BACKING_CAP
    );
    // Voting opens later; before that a vote is refused.
    bea.world = school.world.clone();
    assert!(
        bea.apply(&Action::Vote("ann".into()), &mut school.rng)
            .is_err()
    );
    let vote_day = school.world.term_start + school.content.social.rules.vote_from;
    let (mut bea, _) = school.reopen(&bea.player, vote_day);
    let ballot = bea.ballot();
    assert!(ballot.iter().any(|b| b.id == "npc:bramley") && ballot.iter().any(|b| b.id == "ann"));
    school.act(&mut bea, Action::Vote("ann".into()));
    assert!(
        bea.apply(&Action::Vote("ann".into()), &mut school.rng)
            .is_err(),
        "one vote a term"
    );
    assert!(
        bea.apply(&Action::Vote("nobody".into()), &mut school.rng)
            .is_err()
    );
    assert_eq!(school.world.commons.votes_for("ann"), 1);
    // The count: ann beats the characters who run, and the school changes for everyone.
    let tally = school.world.term_start + school.content.social.rules.tally_day;
    school.tick_to(tally);
    assert_eq!(school.world.commons.election.winner.as_deref(), Some("ann"));
    assert!(school.world.has("head_player"));
    assert!(
        school
            .world
            .paper
            .iter()
            .any(|h| h.text.contains("ann") && h.text.contains("Head of School"))
    );
    // A pupil elsewhere sees the school change (patrols thinner).
    let (cal, _) = Game::open(
        &content(),
        "cal",
        None,
        Some(&school.world.to_bytes()),
        tally,
    );
    let before = Game::open(
        &content(),
        "cal",
        None,
        Some(&World::new(0).to_bytes()),
        tally,
    )
    .0;
    assert!(cal.patrol_level("quad") <= before.patrol_level("quad"));
    // Ann is told, gains legend and the trait; bea is told who won; nothing is counted twice.
    let legend = ann.player.legend;
    let (ann2, events) = school.reopen(&ann.player, tally + 1);
    assert!(
        said(&events).contains("won the school election"),
        "{}",
        said(&events)
    );
    assert!(
        ann2.player.legend
            >= legend + u32::try_from(school.content.social.rules.winner_legend).unwrap()
    );
    assert!(ann2.player.traits.contains(&"head_of_school".to_owned()));
    let (_, events) = school.reopen(&ann2.player, tally + 2);
    assert!(!said(&events).contains("won the school election"));
    let (_, events) = school.reopen(&bea.player, tally + 1);
    assert!(
        said(&events).contains("has chosen ann"),
        "{}",
        said(&events)
    );
    // Once counted, the ballot is closed.
    let (mut late, _) = school.reopen(&rogue.player, tally + 1);
    assert!(late.apply(&Action::Nominate, &mut school.rng).is_err());
}

#[test]
fn with_nobody_standing_a_character_wins_and_the_world_decides_who_runs() {
    let day = monday() + 3;
    let mut school = School::new(day);
    school.enrol("ann", "smooth_talker", day);
    let tally = school.world.term_start + school.content.social.rules.tally_day;
    school.tick_to(tally);
    let winner = school.world.commons.election.winner.clone().unwrap();
    assert!(
        winner.starts_with("npc:"),
        "an empty field goes to a character who runs: {winner}"
    );
    assert!(
        school
            .world
            .has(&format!("head_{}", winner.trim_start_matches("npc:")))
    );
    // The prefects stood down by the ledger story: Bramley is not on the ballot, and cannot win.
    let mut school = School::new(day);
    school.enrol("ann", "smooth_talker", day);
    WorldDelta::Effect {
        id: "prefects_stood_down".into(),
        by: "ann".into(),
        day,
    }
    .apply(&mut school.world, &school.content);
    assert!(
        !russh_bbs_door_hallpass::social::ballot_of(&school.content, &school.world)
            .iter()
            .any(|b| b.id == "npc:bramley")
    );
    school.tick_to(tally);
    let winner = school.world.commons.election.winner.clone().unwrap();
    assert!(
        ["npc:wren", "npc:hollis"].contains(&winner.as_str()),
        "{winner}"
    );
}

#[test]
fn rumours_about_a_candidate_move_their_backing_and_votes_decide_close_races() {
    let c = content();
    let mut world = World::new(0);
    let base = russh_bbs_door_hallpass::social::ballot_of(&c, &world);
    let wren = |b: &[russh_bbs_door_hallpass::social::BallotLine]| {
        b.iter().find(|x| x.id == "npc:wren").unwrap().backing
    };
    // Praise adds, gossip takes away (the template's own number).
    WorldDelta::Rumour {
        id: "fair_play".into(),
        about: "wren".into(),
        by: "ann".into(),
        day: 1,
    }
    .apply(&mut world, &c);
    let praised = russh_bbs_door_hallpass::social::ballot_of(&c, &world);
    assert_eq!(
        wren(&praised),
        wren(&base) + c.rumour("fair_play").unwrap().backing
    );
    // A close race, whoever leads this term: enough votes (each worth `vote_weight`) for the
    // runner-up to overtake the leader, and not before.
    let mut world = World::new(0);
    world.last_day = c.social.rules.tally_day;
    let weight = c.social.rules.vote_weight;
    let mut lines = russh_bbs_door_hallpass::social::ballot_of(&c, &world);
    lines.sort_by(|a, b| b.backing.cmp(&a.backing).then_with(|| a.id.cmp(&b.id)));
    let (leader, second) = (lines[0].clone(), lines[1].clone());
    assert_eq!(
        russh_bbs_door_hallpass::social::tally(&c, &world)
            .unwrap()
            .id,
        leader.id
    );
    let lead = leader.backing - second.backing;
    let needed = usize::try_from(lead / weight + 1).unwrap();
    assert!(needed <= 8, "a close race needs few votes: {needed}");
    for v in 0..needed - 1 {
        WorldDelta::Vote {
            voter: format!("v{v}"),
            who: second.id.clone(),
        }
        .apply(&mut world, &c);
    }
    let short = russh_bbs_door_hallpass::social::tally(&c, &world).unwrap();
    assert_ne!(short.id, second.id, "one vote short");
    WorldDelta::Vote {
        voter: format!("v{}", needed - 1),
        who: second.id.clone(),
    }
    .apply(&mut world, &c);
    assert_eq!(
        russh_bbs_door_hallpass::social::tally(&c, &world)
            .unwrap()
            .id,
        second.id,
        "{needed} votes of {weight} beat a lead of {lead}"
    );
    // A vote for a name that is not on the ballot, and a second vote, are ignored by the world.
    let before = world.commons.election.votes.len();
    WorldDelta::Vote {
        voter: "v0".into(),
        who: "npc:hollis".into(),
    }
    .apply(&mut world, &c);
    WorldDelta::Vote {
        voter: "zed".into(),
        who: "nobody".into(),
    }
    .apply(&mut world, &c);
    assert_eq!(world.commons.election.votes.len(), before);
}

// ---------------------------------------------------------------- awards

#[test]
fn awards_are_scored_from_the_roster_at_term_end_and_paid_to_the_winners_when_they_next_play() {
    let day = monday() + 3;
    let mut school = School::new(day);
    let mut ann = school.enrol("ann", "smooth_talker", day);
    let mut bea = school.enrol("bea", "fast_runner", day);
    ann.player.legend = 40;
    ann.player.pulled = 4;
    bea.player.legend = 12;
    for (_, g) in &mut bea.player.grades {
        *g = 88;
    }
    for (_, g) in &mut ann.player.grades {
        *g = 55;
    }
    school.commit(&mut ann);
    school.commit(&mut bea);
    let term_days = school.content.rules.term_days;
    school.tick_to(school.world.term_start + term_days);
    assert_eq!(school.world.term, 2);
    let wins: Vec<(String, String)> = school
        .world
        .commons
        .awards
        .iter()
        .map(|a| (a.award.clone(), a.name.clone()))
        .collect();
    assert!(
        wins.contains(&("legend_cup".into(), "ann".into())),
        "{wins:?}"
    );
    assert!(wins.contains(&("scholar".into(), "bea".into())), "{wins:?}");
    assert!(wins.contains(&("mischief".into(), "ann".into())));
    // Nobody wins an award they did not score for (the minimums hold).
    assert!(!wins.iter().any(|(a, _)| a == "sleuth"), "{wins:?}");
    // The paper says so.
    assert!(
        school
            .world
            .paper
            .iter()
            .any(|h| h.text.contains("Legend Cup") && h.text.contains("ann"))
    );
    // The winners hear of it, once, when they next play, and the list outlives the term.
    let (ann2, events) = school.reopen(&ann.player, school.world.term_start + 1);
    assert!(said(&events).contains("Legend Cup"), "{}", said(&events));
    assert_eq!(
        ann2.player.term, 2,
        "a new term: a new character, not yet enrolled"
    );
    let owed = ann2.player.social.unpaid;
    assert!(owed >= 2, "the cash is owed until they enrol: {owed}");
    let mut enrolled = ann2.clone();
    enrolled
        .apply(&Action::Enrol("smooth_talker".into()), &mut school.rng)
        .unwrap();
    assert_eq!(
        enrolled.player.cash,
        school.content.rules.start_cash + i32::from(owed) * school.content.social.rules.award_cash
    );
    assert_eq!(enrolled.player.social.unpaid, 0);
    assert!(
        ann2.player
            .social
            .awards
            .iter()
            .any(|a| a.ends_with("legend_cup"))
    );
    let (ann3, events) = school.reopen(&ann2.player, school.world.term_start + 2);
    assert!(!said(&events).contains("Legend Cup"), "not paid twice");
    assert_eq!(
        ann3.player.social.awards.len(),
        ann2.player.social.awards.len()
    );
    // The roster is cleared of last term's numbers, so the next term's awards start from zero.
    assert!(
        school
            .world
            .commons
            .roster
            .iter()
            .all(|p| p.legend == 0 && p.pulled == 0)
    );
    assert!(school.world.commons.rumours.is_empty() && school.world.commons.plots.is_empty());
}

// ---------------------------------------------------------------- through the framework store

fn keys(text: &str) -> Vec<DoorKey> {
    text.chars().map(DoorKey::Char).collect()
}

async fn put_save(store: &MemoryDoorStore, door: &dyn Door, account: i64, player: &Player) {
    store
        .write_save(
            &door.info().id,
            AccountId::from_raw(account),
            &SaveWrite {
                expected: Revision::NONE,
                season: 1,
                version: SAVE_VERSION,
                bytes: &encode_player(player),
            },
        )
        .await
        .unwrap();
}

async fn get_player(store: &MemoryDoorStore, door: &dyn Door, account: i64) -> Player {
    let s = store
        .load_save(&door.info().id, AccountId::from_raw(account))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(s.version, SAVE_VERSION);
    decode_player(&s.bytes).unwrap()
}

async fn get_world(store: &MemoryDoorStore, door: &dyn Door) -> World {
    let w = store.load_world(&door.info().id).await.unwrap().unwrap();
    assert_eq!(w.version, WORLD_VERSION);
    decode_world(&w.bytes).unwrap()
}

/// Two players, two accounts, one board: a scheme from one on the other, through the real store,
/// the real session keys and the real world edit. Then the opt-out, through the same.
#[tokio::test]
async fn two_players_play_a_scheme_through_the_framework_store_and_the_opt_out_stops_it() {
    let store = MemoryDoorStore::new();
    let door: Arc<dyn Door> = Hallpass::shared().unwrap();
    let day = monday() + 3;
    let clock = Arc::new(ManualClock::new(Day::from_raw(day).start()));
    store.set_now(Day::from_raw(day).start());
    let c: Arc<dyn Clock> = clock.clone();
    // Two enrolled pupils saved as the board would hold them.
    let mut school = School::new(day);
    let ann = forger(&mut school, "ann", day).player;
    let bea = school.enrol("bea", "fast_runner", day).player;
    put_save(&store, door.as_ref(), 1, &ann).await;
    put_save(&store, door.as_ref(), 2, &bea).await;
    let env_ann = DoorEnv::for_tests("ann");
    let env_bea = DoorEnv::for_tests("bea");

    // Bea walks in first and so is on the roster; ann then finds her in Classmates.
    let mut run = Running::open(
        &store,
        door.clone(),
        AccountId::from_raw(2),
        &env_bea,
        c.clone(),
        1,
    )
    .await
    .unwrap();
    run.key(DoorKey::Char('o'));
    run.flush(&store).await.unwrap();
    assert!(
        get_world(&store, door.as_ref())
            .await
            .commons
            .pupil("bea")
            .is_some()
    );
    drop(run);

    let mut run = Running::open(
        &store,
        door.clone(),
        AccountId::from_raw(1),
        &env_ann,
        c.clone(),
        2,
    )
    .await
    .unwrap();
    for k in keys("oc") {
        run.key(k);
        run.flush(&store).await.unwrap();
    }
    let screen = run.look_around(&env_ann).join("\n");
    assert!(screen.contains("bea"), "{screen}");
    assert!(screen.contains("Back"), "{screen}");
    // Pick bea, then the first scheme on offer (the swap, since she has a forgery kit).
    for k in keys("11") {
        run.key(k);
        run.flush(&store).await.unwrap();
    }
    let world = get_world(&store, door.as_ref()).await;
    assert_eq!(world.commons.plots.len(), 1, "{:?}", world.commons);
    assert_eq!(world.commons.plots[0].from, "ann");
    assert_eq!(world.commons.plots[0].to, "bea");
    let ann_saved = get_player(&store, door.as_ref(), 1).await;
    assert_eq!(ann_saved.social.sent.len(), 1);
    assert_eq!(ann_saved.nerve, ann.nerve - 2);
    drop(run);

    // Bea opens the next morning: it is delivered, saved, and the world's record is updated.
    clock.set(Day::from_raw(day + 1).start());
    store.set_now(Day::from_raw(day + 1).start());
    let mut run = Running::open(
        &store,
        door.clone(),
        AccountId::from_raw(2),
        &env_bea,
        c.clone(),
        3,
    )
    .await
    .unwrap();
    run.key(DoorKey::Char('o'));
    run.flush(&store).await.unwrap();
    let screen = run.look_around(&env_bea).join("\n");
    assert!(
        screen.contains("swapped") || screen.contains("Lately"),
        "{screen}"
    );
    drop(run);
    let bea_saved = get_player(&store, door.as_ref(), 2).await;
    assert!(bea_saved.social.seen_plot >= 1);
    assert!(bea_saved.social.protected_until > day);
    assert_eq!(bea_saved.cool, bea.cool - 1);
    let world = get_world(&store, door.as_ref()).await;
    assert_eq!(world.commons.plots[0].state, plot_state::LANDED);

    // Ann comes back, hears how it went, and the record leaves the post.
    let mut run = Running::open(
        &store,
        door.clone(),
        AccountId::from_raw(1),
        &env_ann,
        c.clone(),
        4,
    )
    .await
    .unwrap();
    run.key(DoorKey::Char('o'));
    run.flush(&store).await.unwrap();
    drop(run);
    let ann_after = get_player(&store, door.as_ref(), 1).await;
    assert_eq!(ann_after.legend, ann.legend + 1);
    assert!(
        get_world(&store, door.as_ref())
            .await
            .commons
            .plots
            .is_empty()
    );

    // Now bea opts out through the screen. Ann (days later, protection over) is shown her as
    // unreachable, and cannot put a scheme in the post.
    clock.set(Day::from_raw(day + 6).start());
    store.set_now(Day::from_raw(day + 6).start());
    let mut run = Running::open(
        &store,
        door.clone(),
        AccountId::from_raw(2),
        &env_bea,
        c.clone(),
        5,
    )
    .await
    .unwrap();
    for k in keys("oo") {
        run.key(k);
        run.flush(&store).await.unwrap();
    }
    drop(run);
    assert!(get_player(&store, door.as_ref(), 2).await.social.opt_out);
    assert!(
        get_world(&store, door.as_ref())
            .await
            .commons
            .opted_out("bea")
    );
    let mut run = Running::open(
        &store,
        door.clone(),
        AccountId::from_raw(1),
        &env_ann,
        c.clone(),
        6,
    )
    .await
    .unwrap();
    for k in keys("oc") {
        run.key(k);
        run.flush(&store).await.unwrap();
    }
    let screen = run.look_around(&env_ann).join("\n");
    assert!(screen.contains("opted out"), "{screen}");
    for k in keys("11") {
        run.key(k);
        run.flush(&store).await.unwrap();
    }
    assert!(
        get_world(&store, door.as_ref())
            .await
            .commons
            .plots
            .is_empty(),
        "nothing was posted for an opted-out pupil"
    );
}
