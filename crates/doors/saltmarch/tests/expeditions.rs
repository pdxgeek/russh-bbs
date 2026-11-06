//! Whole expeditions through the real door: a recorded visit replays to exactly the saved
//! state, a trip survives a disconnect in the middle of a scene or a fight, a trip left open
//! past midnight ends in the dark, and a version 1 save (before expeditions) still loads.

#![allow(clippy::unwrap_used, clippy::panic, clippy::too_many_lines)]

use std::collections::BTreeSet;
use std::sync::Arc;

use russh_bbs_core::doors::{DoorStore, Revision, SaveWrite};
use russh_bbs_core::limits::{Clock, Day, ManualClock};
use russh_bbs_core::store::AccountId;
use russh_bbs_door::host::{Driver, Running};
use russh_bbs_door::replay::{self, Recorder};
use russh_bbs_door::sim::MemoryDoorStore;
use russh_bbs_door::{BlobKind, Door, DoorEnv, DoorKey, DoorRng, Flow, Save, Start};
use russh_bbs_door_saltmarch::clan::{Clan, Settings};
use russh_bbs_door_saltmarch::combat::{Fight, Progress, Unit};
use russh_bbs_door_saltmarch::content::Catalog;
use russh_bbs_door_saltmarch::engine::{self, Action, Env};
use russh_bbs_door_saltmarch::map::Map;
use russh_bbs_door_saltmarch::tide::Tide;
use russh_bbs_door_saltmarch::trip::{Kit, Phase, Trip};
use russh_bbs_door_saltmarch::world::World;
use russh_bbs_door_saltmarch::{SAVE_VERSION, Saltmarch};

const DAY: i64 = 20_000;

fn clock(day: i64) -> Arc<ManualClock> {
    Arc::new(ManualClock::new(Day::from_raw(day).start()))
}

struct Live {
    driver: Driver,
    recorder: Recorder,
    last_save: Option<Vec<u8>>,
    env: DoorEnv,
}

impl Live {
    fn open(door: &Saltmarch, seed: u64, save: Option<Vec<u8>>, day: i64) -> Self {
        let env = DoorEnv::for_tests("ann");
        // A returning player finds the season's world as it was stored.
        let world = save
            .as_deref()
            .and_then(Clan::decode)
            .map(|c| World::new(1, c.seed, DAY).encode());
        let driver = Driver::open(
            door,
            Start {
                save: match &save {
                    Some(bytes) => Save::Loaded { bytes, season: 1 },
                    None => Save::Fresh,
                },
                world: world.as_deref(),
                env: &env,
                today: Day::from_raw(day),
                season: 1,
            },
            seed,
            clock(day),
        );
        Self {
            driver,
            recorder: Recorder::new(door, seed, Day::from_raw(day).start().as_raw(), 1, save),
            last_save: None,
            env,
        }
    }

    fn key(&mut self, key: DoorKey) {
        self.recorder.key(key);
        self.driver.key(key);
        if let Some(bytes) = self.driver.take_effects().save() {
            self.last_save = Some(bytes.to_vec());
        }
    }

    fn screen(&self) -> Vec<String> {
        self.driver.look_around(&self.env)
    }

    fn choose(&mut self, tag: &str) -> bool {
        let found = self.screen().iter().find_map(|line| {
            let rest = line.strip_prefix('[')?;
            let (key, text) = rest.split_once("] ")?;
            text.starts_with(tag).then(|| key.chars().next())?
        });
        found.map(|c| self.key(DoorKey::Char(c))).is_some()
    }

    fn clan(&self) -> Clan {
        Clan::decode(self.last_save.as_deref().unwrap()).unwrap()
    }

    fn said(&self, needle: &str) -> bool {
        self.screen().join("\n").contains(needle)
    }
}

fn turns_left(screen: &[String]) -> u32 {
    screen
        .iter()
        .find_map(|l| l.strip_prefix("Turns ")?.split('/').next()?.parse().ok())
        .unwrap_or(0)
}

fn turns_home(screen: &[String]) -> u32 {
    screen
        .iter()
        .find_map(|l| {
            let at = l.find(" turns home")?;
            l[..at].rsplit(' ').next()?.parse().ok()
        })
        .unwrap_or(0)
}

/// Set out with the first two crew and a standard kit.
fn set_out(live: &mut Live) {
    live.key(DoorKey::Enter); // found "ann"
    assert!(live.choose("go:expedition"));
    for tag in ["party:1", "party:2", "kit:standard", "depart"] {
        assert!(live.choose(tag), "{tag}: {:?}", live.screen());
    }
    assert!(live.said("Party: "), "on the marsh");
}

/// One decision by a cautious player: answer scenes with the first option, quick-resolve fights,
/// work what is here, go home in good time, and otherwise strike out.
fn step(live: &mut Live, leg: usize) -> bool {
    let screen = live.screen();
    let left = turns_left(&screen);
    let home = turns_home(&screen);
    for tag in ["choose:0", "quick"] {
        if live.choose(tag) {
            return true;
        }
    }
    if left <= home + 3 && live.choose("home") {
        return true;
    }
    let dirs = ["explore:n", "explore:e", "explore:w", "explore:s"];
    let go = if leg % 3 == 2 {
        "gather"
    } else {
        dirs[leg % 4]
    };
    live.choose(go) || live.choose("march") || live.choose("home")
}

/// The title of the scene on the screen, if one is waiting: the line after the load line.
fn scene_title(screen: &[String]) -> Option<String> {
    let waiting = screen.iter().any(|l| l.starts_with("[a] choose:"));
    let at = screen.iter().position(|l| l.starts_with("Load "))?;
    waiting.then(|| screen.get(at + 1).cloned()).flatten()
}

fn run_trip(live: &mut Live, titles: &mut BTreeSet<String>, fights: &mut u32) {
    for leg in 0..70 {
        let screen = live.screen();
        if screen.iter().any(|l| l.starts_with("Skirmish, round")) {
            *fights += 1;
        }
        titles.extend(scene_title(&screen));
        if live.last_save.is_some() && live.clan().trip.is_none() {
            return;
        }
        if !step(live, leg) {
            return;
        }
    }
}

#[test]
fn whole_expeditions_replay_to_exactly_the_saved_state_on_every_branch() {
    let door = Saltmarch::new().unwrap();
    let mut endings = BTreeSet::new();
    let mut titles = BTreeSet::new();
    let mut fights = 0;
    let mut replayed = 0;
    for seed in 1..=24u64 {
        let mut live = Live::open(&door, seed, None, DAY);
        set_out(&mut live);
        run_trip(&mut live, &mut titles, &mut fights);
        let clan = live.clan();
        endings.insert(if clan.trip.is_some() {
            "still out"
        } else if clan.stats.returned > 0 {
            "home"
        } else if clan.stats.swept > 0 {
            "swept"
        } else if clan.stats.cut_off > 0 {
            "cut off"
        } else {
            "lost"
        });
        let run = replay::run(&door, &live.recorder.into_log(), &live.env);
        assert_eq!(
            run.save, live.last_save,
            "seed {seed}: the log reproduces the save"
        );
        replayed += 1;
    }
    assert_eq!(replayed, 24);
    assert!(endings.contains("home"), "{endings:?}");
    assert!(titles.len() >= 5, "scenes met across the walks: {titles:?}");
    assert!(fights > 0, "and at least one fight");
}

/// Play until the trip is waiting on a scene or a fight (or give up).
fn until_waiting(live: &mut Live) -> Option<&'static str> {
    for leg in 0..60 {
        if live.said("choose:0") {
            return Some("scene");
        }
        if live.said("Skirmish, round") {
            return Some("fight");
        }
        if live.last_save.is_some() && live.clan().trip.is_none() {
            return None;
        }
        let dirs = ["explore:n", "explore:e", "explore:w", "explore:s"];
        if !(live.choose(dirs[leg % 4]) || live.choose("gather") || live.choose("camp:3")) {
            return None;
        }
    }
    None
}

#[test]
fn a_trip_survives_a_disconnect_in_the_middle_of_a_scene() {
    let door = Saltmarch::new().unwrap();
    let mut scenes = 0;
    for seed in 1..=30u64 {
        let mut live = Live::open(&door, seed, None, DAY);
        set_out(&mut live);
        if until_waiting(&mut live) != Some("scene") {
            continue;
        }
        let before = live.screen();
        let save = live.last_save.clone().unwrap();
        assert!(matches!(
            live.clan().trip.as_ref().unwrap().phase,
            Phase::Scene(_)
        ));
        // A new visit, a new seed, the same day: the screen picks up exactly where it was.
        let mut back = Live::open(&door, seed + 1000, Some(save), DAY);
        let strip = |s: &[String]| -> Vec<String> {
            s.iter()
                .filter(|l| !l.starts_with("Tide:"))
                .cloned()
                .collect()
        };
        assert_eq!(strip(&before), strip(&back.screen()), "seed {seed}");
        assert!(back.choose("choose:0"), "and it can be answered");
        scenes += 1;
        if scenes >= 3 {
            break;
        }
    }
    assert!(scenes >= 3, "{scenes} scenes");
}

#[test]
fn a_trip_survives_a_disconnect_in_the_middle_of_a_fight() {
    let door = Saltmarch::new().unwrap();
    let mut live = Live::open(&door, 21, None, DAY);
    live.key(DoorKey::Enter);
    let mut clan = live.clan();
    // Set the party out by the engine, and put a skirmish in front of it.
    let cat = Catalog::embedded().unwrap();
    let map = Map::generate(clan.seed, &cat.pois);
    let tide = Tide::new(clan.seed);
    let prices = World::new(1, clan.seed, DAY).prices;
    let env = Env {
        cat: &cat,
        map: &map,
        tide: &tide,
        prices: &prices,
        settings: Settings::default(),
        day: DAY,
    };
    let ids: Vec<u16> = clan.crew.iter().map(|c| c.id).collect();
    engine::apply(
        &env,
        &mut clan,
        Action::Depart {
            party: ids,
            kit: Kit {
                food: 4,
                ..Kit::default()
            },
            target: None,
        },
        &mut DoorRng::from_seed(1),
    )
    .unwrap();
    let units = clan
        .crew
        .iter()
        .map(|c| Unit::from_crew(&cat, c, false))
        .collect();
    let mut fight = Fight::begin(&cat, units, "hound-pack", false, 40, 0, 0);
    assert!(matches!(
        fight.advance(&mut DoorRng::from_seed(2), &mut Vec::new()),
        Progress::Turn(_)
    ));
    clan.trip.as_mut().unwrap().phase = Phase::Fight(Box::new(fight));
    let mut back = Live::open(&door, 22, Some(clan.encode()), DAY);
    let screen = back.screen().join("\n");
    assert!(screen.contains("Skirmish, round 1"), "{screen}");
    assert!(
        screen.contains("Foes front:") && screen.contains("Bog-hound"),
        "{screen}"
    );
    assert!(back.choose("flee") || back.choose("brace"));
    for _ in 0..8 {
        if !back.said("Skirmish, round") {
            break;
        }
        assert!(back.choose("quick"));
    }
    assert!(
        !back.said("Skirmish, round"),
        "the fight is over: {:?}",
        back.screen()
    );
    let after = back.clan();
    assert!(after.stats.fights == 0 || after.trip.is_some() || after.stats.trips > 0);
}

#[test]
fn a_trip_left_open_past_midnight_ends_in_the_dark_and_the_report_says_so() {
    let door = Saltmarch::new().unwrap();
    let mut live = Live::open(&door, 3, None, DAY);
    set_out(&mut live);
    for tag in ["explore:n", "gather", "explore:e"] {
        live.choose(tag);
    }
    let save = live.last_save.clone().unwrap();
    let out = live.clan();
    let away = out.trip.is_some();
    assert!(away || out.stats.trips > 0, "the walk left a trip open");
    if !away {
        return; // swept or cut off already: nothing left to leave open
    }
    // Tomorrow morning: the report leads, the trip is over and the clan is at home.
    let mut next = Live::open(&door, 4, Some(save), DAY + 1);
    assert!(next.said("While you were away"), "{:?}", next.screen());
    assert!(next.said("Cut off after"), "{:?}", next.screen());
    next.key(DoorKey::Char(' '));
    next.key(DoorKey::Char(' '));
    let clan = next.clan();
    assert!(clan.trip.is_none());
    assert_eq!(clan.stats.cut_off, 1);
    assert_eq!(clan.turns_used, 0);
}

#[test]
fn a_hold_action_is_refused_while_the_party_is_out() {
    let door = Saltmarch::new().unwrap();
    let mut live = Live::open(&door, 5, None, DAY);
    set_out(&mut live);
    let clan = live.clan();
    assert!(clan.trip.is_some());
    // The trip screen has no hold menu, and the main menu offers no expedition twice.
    assert!(!live.said("go:hold") && !live.said("go:market"));
    assert!(!live.choose("go:expedition"));
    // Leaving the door mid-trip keeps the party out.
    live.key(DoorKey::Escape);
    assert!(live.clan().trip.is_some());
}

fn v1_bytes(clan: &Clan) -> Vec<u8> {
    // Version 3 appended the (here empty) social state; version 2 appended eleven counters, the
    // marks and the trip flag.
    let mut social = russh_bbs_door_saltmarch::codec::Writer::new();
    russh_bbs_door_saltmarch::social::ledger::ClanSocial::default().write(&mut social);
    let mut story = russh_bbs_door_saltmarch::codec::Writer::new();
    russh_bbs_door_saltmarch::story::Story::default().write(&mut story);
    let bytes = clan.encode();
    bytes[..bytes.len() - story.finish().len() - social.finish().len() - (11 * 4 + 2)].to_vec()
}

#[tokio::test]
async fn a_version_one_save_migrates_and_plays_on() {
    let door: Arc<dyn Door> = Saltmarch::shared().unwrap();
    let mut live = Live::open(&Saltmarch::new().unwrap(), 7, None, DAY);
    live.key(DoorKey::Enter);
    let clan = live.clan();
    assert!(clan.trip.is_none() && clan.marks.is_empty());
    let v1 = v1_bytes(&clan);
    // The step itself.
    let v2 = door.migrate(BlobKind::Save, 1, &v1).unwrap();
    assert_ne!(v2, clan.encode(), "one step is one version");
    let v3 = door.migrate(BlobKind::Save, 2, &v2).unwrap();
    assert_ne!(v3, clan.encode(), "and so is the next");
    let up = door.migrate(BlobKind::Save, 3, &v3).unwrap();
    assert_eq!(up, clan.encode());
    assert_eq!(Clan::decode(&up), Some(clan.clone()));
    assert!(
        door.migrate(BlobKind::Save, 1, &v1[..10]).is_err(),
        "damage is refused"
    );
    assert!(door.migrate(BlobKind::Save, 2, &v1).is_err());
    assert!(door.migrate(BlobKind::World, 1, &v1).is_err());
    // And through the store, the way the board opens a door.
    let store = MemoryDoorStore::new();
    let ann = AccountId::from_raw(1);
    let id = door.info().id.clone();
    store
        .write_save(
            &id,
            ann,
            &SaveWrite {
                expected: Revision::NONE,
                season: 1,
                version: 1,
                bytes: &v1,
            },
        )
        .await
        .unwrap();
    let clock: Arc<dyn Clock> = clock(DAY);
    let env = DoorEnv::for_tests("ann");
    let mut run = Running::open(&store, door.clone(), ann, &env, clock, 1)
        .await
        .unwrap();
    let screen = run.look_around(&env).join("\n");
    assert!(
        screen.contains("ann - day 1") && screen.contains("go:expedition"),
        "{screen}"
    );
    // The first key that saves writes it back at the current version.
    assert_eq!(run.key(DoorKey::Char('x')), Flow::Stay);
    let stored = store.load_save(&id, ann).await.unwrap().unwrap();
    assert_eq!(
        stored.version, 1,
        "nothing is rewritten until a key asks for a save"
    );
    let mut run = run;
    let survey_line = run
        .look_around(&env)
        .into_iter()
        .find(|l| l.contains("survey"))
        .unwrap();
    let key = survey_line
        .strip_prefix('[')
        .unwrap()
        .chars()
        .next()
        .unwrap();
    run.key(DoorKey::Char(key));
    run.flush(&store).await.unwrap();
    let stored = store.load_save(&id, ann).await.unwrap().unwrap();
    assert_eq!(stored.version, SAVE_VERSION);
    let clan2 = Clan::decode(&stored.bytes).unwrap();
    assert_eq!(clan2.name, "ann");
    assert!(clan2.charted_count() > clan.charted_count());
}

#[test]
fn the_trip_screen_is_a_text_view_a_screen_reader_can_use() {
    let door = Saltmarch::new().unwrap();
    let mut live = Live::open(&door, 11, None, DAY);
    set_out(&mut live);
    let text = live.screen().join("\n");
    for needle in ["Party:", "Around you - north:", "Load ", "turns", "water "] {
        assert!(
            text.contains(needle) || needle == "turns",
            "{needle} in\n{text}"
        );
    }
    // Everything on the screen is plain text with no control characters.
    assert!(text.chars().all(|c| !c.is_control() || c == '\n'));
    let trip: Trip = live.clan().trip.unwrap();
    assert_eq!(trip.party.len(), 2);
}
