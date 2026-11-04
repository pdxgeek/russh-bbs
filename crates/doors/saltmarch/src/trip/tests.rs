//! Expedition rules: the route and the tide, the flood and the night, cargo, gathering, scene
//! selection, and the RPG rule: the same scene shows a different list to a different party, a
//! scar closes one door and opens another, history decides what is drawn, and skill, luck and
//! steadiness move the odds. Each refusal leaves the clan untouched.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::too_many_lines,
    clippy::many_single_char_names,
    clippy::default_trait_access,
    clippy::needless_pass_by_value
)]

use super::*;
use crate::clan::Settings;
use crate::combat::FightAction;
use crate::crew::Injured;
use crate::lore::{Cond, Effect, Req, Water};
use crate::map::Map;
use crate::tide::Tide;
use crate::world::{Prices, World};

const DAY: i64 = 20_000;

fn rng(seed: u64) -> DoorRng {
    DoorRng::from_seed(seed)
}

/// The session's seed only starts the trip's dice; tests start them from a fixed one.
fn depart(
    env: &Env<'_>,
    clan: &mut Clan,
    party: &[u16],
    kit: Kit,
    target: Option<(u8, u8)>,
    events: &mut Vec<Event>,
) -> Result<(), ActionError> {
    super::depart(env, clan, party, kit, target, &mut rng(77), events)
}

/// A trip's dice are in its save; tests that want a particular roll put the generator they
/// hold into the trip first, and take the dice back after, so a run of calls goes on from one
/// stream.
fn apply(
    env: &Env<'_>,
    clan: &mut Clan,
    act: TripAction,
    session: &mut DoorRng,
) -> Result<Vec<Event>, ActionError> {
    let old = clan.trip.as_ref().map(|t| t.rng);
    if let Some(t) = clan.trip.as_mut() {
        t.rng = session.state();
    }
    let result = super::apply(env, clan, act);
    match (&result, clan.trip.as_mut(), old) {
        // A refusal rolls nothing: the trip is exactly as it was.
        (Err(_), Some(t), Some(old)) => t.rng = old,
        (_, Some(t), _) => *session = DoorRng::from_state(t.rng),
        _ => {}
    }
    result
}

struct Fx {
    cat: Catalog,
    map: Map,
    tide: Tide,
    prices: Prices,
    settings: Settings,
}

impl Fx {
    fn new() -> Self {
        let cat = Catalog::embedded().unwrap();
        // The first seed whose marsh has every sort of place.
        let (seed, map) = (11u64..2000)
            .map(|s| (s, Map::generate(s, &cat.pois)))
            .find(|(_, m)| {
                [
                    PoiKind::Camp,
                    PoiKind::Shrine,
                    PoiKind::Salvage,
                    PoiKind::Lair,
                    PoiKind::Hermit,
                    PoiKind::Trader,
                    PoiKind::Ruin,
                ]
                .iter()
                .all(|k| m.pois.iter().any(|p| p.kind == *k))
            })
            .unwrap();
        Self {
            tide: Tide::new(seed),
            prices: World::new(1, seed, 0).prices,
            settings: Settings::default(),
            cat,
            map,
        }
    }

    /// The same marsh with no scenes, so a march meets nothing.
    fn quiet() -> Self {
        let mut f = Self::new();
        f.cat.events.clear();
        f
    }

    fn env(&self, day: i64) -> Env<'_> {
        Env {
            cat: &self.cat,
            map: &self.map,
            tide: &self.tide,
            prices: &self.prices,
            settings: self.settings,
            day,
        }
    }

    /// Six crew with no traits: wader, raker, salvager, fighter, healer, reed-witch.
    fn clan(&self) -> Clan {
        let mut clan = Clan::found(&self.cat, &self.map, "Test", 1, 0, DAY, &mut rng(1)).0;
        clan.hold = crate::hold::Hold::default();
        let trades = [
            Trade::Wader,
            Trade::Raker,
            Trade::Salvager,
            Trade::Fighter,
            Trade::Healer,
            Trade::ReedWitch,
        ];
        clan.crew.truncate(3);
        while clan.crew.len() < 6 {
            let mut c = Crew::generate(&self.cat, &mut rng(40 + clan.crew.len() as u64), 0, None);
            c.id = clan.take_id();
            clan.crew.push(c);
        }
        for (c, t) in clan.crew.iter_mut().zip(trades) {
            c.trade = t;
            c.xp = [0; crate::types::TRADES];
            c.xp[t.index()] = 4;
            c.attrs = [5, 5, 5];
            c.traits.clear();
            c.injuries.clear();
            c.age = 30;
            c.post = None;
        }
        clan.tallies = 100;
        clan.stock = Stock::default();
        for r in Resource::ALL {
            clan.stock.add(r, 100);
        }
        clan
    }

    fn home(&self, clan: &Clan) -> (u8, u8) {
        home(&self.env(DAY), clan)
    }

    /// The first (day, turn) where `want(level)` holds, searching a fortnight.
    fn moment(&self, want: impl Fn(i32) -> bool) -> (i64, u8) {
        for day in DAY..DAY + 14 {
            for turn in 0..26u8 {
                let minute = Tide::turn_minute(day, turn, 30);
                if want(self.tide.level(minute)) {
                    return (day, turn);
                }
            }
        }
        panic!("no such moment");
    }

    /// The highest tile of a kind that is not the hold: safe from all but the worst floods.
    fn tile_of(&self, kind: TileKind, clan: &Clan) -> (u8, u8) {
        let home = self.home(clan);
        self.map
            .iter()
            .filter(|((x, y), t)| {
                t.kind == kind && (u8::try_from(*x).unwrap(), u8::try_from(*y).unwrap()) != home
            })
            .max_by_key(|((x, y), t)| (t.elevation, std::cmp::Reverse((*x, *y))))
            .map(|((x, y), _)| (u8::try_from(x).unwrap(), u8::try_from(y).unwrap()))
            .unwrap()
    }
}

fn party(clan: &Clan, n: usize) -> Vec<u16> {
    clan.crew.iter().take(n).map(|c| c.id).collect()
}

fn out(fx: &Fx, clan: &mut Clan, ids: &[u16], kit: Kit) {
    depart(&fx.env(DAY), clan, ids, kit, None, &mut Vec::new()).unwrap();
}

fn kit() -> Kit {
    Kit {
        food: 4,
        rope: 2,
        peat: 2,
        tools: 1,
        ..Kit::default()
    }
}

// ------------------------------------------------------------------ setting out and coming home

#[test]
fn departing_pays_the_kit_and_a_refusal_changes_nothing() {
    let fx = Fx::new();
    let env = fx.env(DAY);
    let clan = fx.clan();
    let ids = party(&clan, 6);
    let refuse = |c: &Clan, party: &[u16], kit: Kit, want: ActionError| {
        let mut c = c.clone();
        let before = c.clone();
        let err = depart(&env, &mut c, party, kit, None, &mut Vec::new()).unwrap_err();
        assert_eq!(err, want);
        assert_eq!(c, before, "a refusal changes nothing");
    };
    refuse(&clan, &[], kit(), ActionError::BadParty);
    refuse(&clan, &ids, kit(), ActionError::BadParty);
    refuse(&clan, &[ids[0], ids[0]], kit(), ActionError::BadParty);
    refuse(&clan, &[999], kit(), ActionError::NoCrew(999));
    let mut hurt = clan.clone();
    hurt.crew[0].injuries.push(Injured {
        id: "broken-leg".into(),
        days: 5,
    });
    let name = hurt.crew[0].name.clone();
    refuse(&hurt, &ids[..2], kit(), ActionError::Unfit(name));
    refuse(
        &clan,
        &ids[..2],
        Kit { boats: 3, ..kit() },
        ActionError::BadKit("at most 2 boats".into()),
    );
    let mut poor = clan.clone();
    poor.stock.take_up_to(Resource::Food, 100);
    refuse(
        &poor,
        &ids[..2],
        kit(),
        ActionError::CantAfford("4 preserved food".into()),
    );
    let mut rich = clan.clone();
    rich.stock.add(Resource::Food, 200);
    refuse(
        &rich,
        &ids[..1],
        Kit {
            food: 250,
            ..Kit::default()
        },
        ActionError::Overweight,
    );
    let mut late = clan.clone();
    late.turns_used = 27;
    refuse(
        &late,
        &ids[..2],
        kit(),
        ActionError::NoTurns { need: 4, have: 3 },
    );

    let mut c = clan.clone();
    let mut events = Vec::new();
    depart(&env, &mut c, &ids[..2], kit(), None, &mut events).unwrap();
    assert_eq!(events, vec![Event::Departed { crew: 2 }]);
    assert_eq!(c.stock.get(Resource::Food), 96);
    assert_eq!(c.stock.get(Resource::Tools), 99);
    assert_eq!(c.stats.departures, 1);
    let t = c.trip.as_ref().unwrap();
    assert_eq!((t.at, t.party.clone()), (fx.home(&c), ids[..2].to_vec()));
    assert_eq!(
        depart(&env, &mut c, &ids[2..3], kit(), None, &mut Vec::new()),
        Err(ActionError::OnTrip)
    );
}

#[test]
fn a_fearful_hand_will_not_set_out_when_the_flood_is_up() {
    let fx = Fx::new();
    let mut clan = fx.clan();
    clan.crew[0].traits.push("fearful".into());
    let (day, turn) = fx.moment(|l| l >= crate::tide::HOLD_FLOOD_LEVEL);
    clan.turns_used = turn;
    let env = fx.env(day);
    let ids = party(&clan, 2);
    let err = depart(&env, &mut clan, &ids, kit(), None, &mut Vec::new()).unwrap_err();
    assert!(matches!(err, ActionError::Refuses(_)), "{err:?}");
    let (day, turn) = fx.moment(|l| l < 40);
    clan.turns_used = turn;
    assert!(depart(&fx.env(day), &mut clan, &ids, kit(), None, &mut Vec::new()).is_ok());
}

#[test]
fn coming_home_returns_the_unspent_supplies_and_the_cargo() {
    let fx = Fx::new();
    let env = fx.env(DAY);
    let mut clan = fx.clan();
    let ids = party(&clan, 2);
    out(&fx, &mut clan, &ids, kit());
    let home = fx.home(&clan);
    {
        let t = clan.trip.as_mut().unwrap();
        t.loot.add(Resource::Reed, 7);
        t.tallies = 9;
    }
    let events = apply(
        &env,
        &mut clan,
        TripAction::March { to: Some(home) },
        &mut rng(1),
    )
    .unwrap();
    assert!(clan.trip.is_none());
    assert_eq!(
        clan.stock.get(Resource::Food),
        100,
        "no turns, no rations eaten"
    );
    assert_eq!(clan.stock.get(Resource::Reed), 107);
    assert_eq!(clan.tallies, 109);
    assert_eq!((clan.stats.trips, clan.stats.returned), (1, 1));
    assert!(events.iter().any(|e| matches!(
        e,
        Event::TripEnded {
            ending: Ending::Home,
            value: 23,
            turns: 0
        }
    )));
    // Nothing to do on a trip that is over.
    assert_eq!(
        apply(&env, &mut clan, TripAction::Camp { turns: 1 }, &mut rng(1)),
        Err(ActionError::NoTrip)
    );
}

#[test]
fn rations_and_lamp_peat_are_eaten_by_the_hours_out() {
    let fx = Fx::quiet();
    let env = fx.env(DAY);
    let mut clan = fx.clan();
    let ids = party(&clan, 3);
    out(
        &fx,
        &mut clan,
        &ids,
        Kit {
            food: 4,
            peat: 3,
            ..Kit::default()
        },
    );
    let home = fx.home(&clan);
    apply(&env, &mut clan, TripAction::Camp { turns: 8 }, &mut rng(1)).unwrap();
    {
        let t = clan.trip.as_ref().unwrap();
        assert_eq!(t.turns_out, 8);
        assert_eq!(
            t.night_turns, 5,
            "the first five turns are before four in the morning"
        );
        // Three crew for eight turns is one ration (a ration is a day out for one); five night
        // turns burn one peat.
        assert_eq!(t.rations_due(), 1);
        assert!(!t.hungry());
    }
    apply(
        &env,
        &mut clan,
        TripAction::March { to: Some(home) },
        &mut rng(1),
    )
    .unwrap();
    assert_eq!(clan.stock.get(Resource::Food), 100 - 4 + 3);
    assert_eq!(clan.stock.get(Resource::Peat), 100 - 3 + 2);
    // A party that eats more than it brought is hungry.
    let mut starved = fx.clan();
    out(&fx, &mut starved, &ids, Kit::default());
    starved.trip.as_mut().unwrap().turns_out = 30;
    assert!(starved.trip.as_ref().unwrap().hungry());
}

// ------------------------------------------------------------------ the route and the water

#[test]
fn a_route_never_enters_ground_the_water_will_have_by_then() {
    let fx = Fx::quiet();
    let clan = fx.clan();
    let mut tried = 0;
    let mut reached = 0;
    for (day, turn) in [
        fx.moment(|l| l < 30),
        fx.moment(|l| l > 85),
        fx.moment(|l| (45..55).contains(&l)),
    ] {
        let env = fx.env(day);
        let mut c = clan.clone();
        c.turns_used = turn;
        let ids = party(&c, 3);
        depart(&env, &mut c, &ids, kit(), None, &mut Vec::new()).unwrap();
        let trip = c.trip.clone().unwrap();
        for dy in (-12..=12i16).step_by(3) {
            for dx in (-12..=12i16).step_by(3) {
                let home = fx.home(&c);
                let (Ok(x), Ok(y)) = (
                    u8::try_from(i16::from(home.0) + dx),
                    u8::try_from(i16::from(home.1) + dy),
                ) else {
                    continue;
                };
                if (x, y) == fx.home(&c) {
                    continue;
                }
                tried += 1;
                let Some((path, total)) = route(&env, &c, &trip, (x, y)) else {
                    continue;
                };
                assert!(total <= u32::from(c.turns_left(&env.settings)));
                if total >= u32::from(c.turns_left(&env.settings)) {
                    continue;
                }
                reached += 1;
                let mut walker = c.clone();
                let events = apply(
                    &env,
                    &mut walker,
                    TripAction::March { to: Some((x, y)) },
                    &mut rng(3),
                )
                .unwrap();
                let at = walker.trip.as_ref().map(|t| t.at);
                assert_eq!(at, Some((x, y)), "the march arrives: {events:?}");
                assert_eq!(
                    u32::from(walker.turns_used - c.turns_used),
                    total,
                    "and spends what the route said"
                );
                assert_eq!(walker.stats.swept, 0);
                assert_eq!(path.last(), Some(&(x, y)));
            }
        }
    }
    assert!(reached > 20 && tried > reached, "{reached} of {tried}");
}

#[test]
fn what_a_step_costs_is_the_tile_plus_the_load_and_the_dark() {
    let fx = Fx::quiet();
    let mut clan = fx.clan();
    let ids = party(&clan, 2);
    // Noon, so the dark does not matter yet.
    let (day, turn) = fx.moment(|_| true);
    let _ = (day, turn);
    out(&fx, &mut clan, &ids, kit());
    let env = fx.env(DAY);
    let home = fx.home(&clan);
    let to = Dir::ALL
        .into_iter()
        .filter_map(|d| d.step(home))
        .find(|t| {
            let minute = env.minute(&clan);
            move_cost(&env, &clan, clan.trip.as_ref().unwrap(), *t, minute).is_some()
        })
        .unwrap();
    let noon = Tide::turn_minute(DAY, 15, 30);
    let night = Tide::turn_minute(DAY, 1, 30);
    let base = |t: &Trip, minute| move_cost(&env, &clan, t, to, minute);
    let lit = clan.trip.clone().unwrap();
    let mut dark = lit.clone();
    dark.kit.peat = 0;
    assert_eq!(
        base(&lit, noon),
        base(&dark, noon),
        "daylight needs no lamp"
    );
    assert_eq!(
        base(&dark, night).unwrap(),
        base(&lit, night).unwrap() + 1,
        "night without peat costs a turn more"
    );
    // Too much cargo: a turn more per step; a boat's room takes it away again.
    let mut heavy = lit.clone();
    let cap = heavy.capacity(&clan, &fx.cat);
    heavy.loot.add(Resource::Reed, cap);
    assert!(heavy.overloaded(&clan, &fx.cat));
    assert_eq!(base(&heavy, noon).unwrap(), base(&lit, noon).unwrap() + 1);
    heavy.kit.boats = 1;
    assert!(!heavy.overloaded(&clan, &fx.cat), "a boat carries the load");
    assert_eq!(heavy.capacity(&clan, &fx.cat), cap + 12);
}

#[test]
fn only_a_boat_enters_drowned_ground_and_a_boat_cannot_be_swept() {
    let fx = Fx::quiet();
    let mut clan = fx.clan();
    let ids = party(&clan, 2);
    let (day, turn) = fx.moment(|l| l >= 70);
    clan.turns_used = turn;
    let env = fx.env(day);
    let low = fx
        .map
        .iter()
        .find(|(_, t)| i32::from(t.elevation) + 14 < 70 && t.kind != TileKind::Deep)
        .map(|((x, y), _)| (u8::try_from(x).unwrap(), u8::try_from(y).unwrap()))
        .unwrap();
    depart(&env, &mut clan, &ids, kit(), None, &mut Vec::new()).unwrap();
    let minute = env.minute(&clan);
    let mut boat = clan.trip.clone().unwrap();
    assert_eq!(move_cost(&env, &clan, &boat, low, minute), None);
    boat.kit.boats = 1;
    assert!(move_cost(&env, &clan, &boat, low, minute).is_some());
    // On a boat the safe level is gone.
    assert_eq!(safe_level(&env, &boat), None);
}

#[test]
fn the_flood_sweeps_a_party_camped_on_low_ground_and_a_boat_saves_it() {
    let fx = Fx::quiet();
    let clan0 = fx.clan();
    // A tile that is dry at some low water and covered within ten turns.
    let mut found = None;
    'search: for day in DAY..DAY + 14 {
        for turn in 0..20u8 {
            let minute = Tide::turn_minute(day, turn, 30);
            let now = fx.tide.level(minute);
            let later = fx.tide.level(Tide::turn_minute(day, turn + 10, 30));
            if now <= 35 && later >= 85 {
                for ((x, y), t) in fx.map.iter() {
                    let e = i32::from(t.elevation);
                    if e >= now && e + 14 < later && e + 12 >= now + 6 {
                        found = Some((
                            day,
                            turn,
                            (u8::try_from(x).unwrap(), u8::try_from(y).unwrap()),
                        ));
                        break 'search;
                    }
                }
            }
        }
    }
    let (day, turn, tile) = found.unwrap();
    let env = fx.env(day);
    let run = |boats: u8| {
        let mut clan = clan0.clone();
        clan.turns_used = turn;
        let ids = party(&clan, 3);
        depart(
            &env,
            &mut clan,
            &ids,
            Kit {
                boats,
                food: 4,
                ..Kit::default()
            },
            None,
            &mut Vec::new(),
        )
        .unwrap();
        {
            let t = clan.trip.as_mut().unwrap();
            t.at = tile;
            t.loot.add(Resource::Reed, 10);
            t.tallies = 10;
        }
        let events = apply(&env, &mut clan, TripAction::Camp { turns: 10 }, &mut rng(2)).unwrap();
        (clan, events)
    };
    let (swept, events) = run(0);
    assert!(swept.trip.is_none());
    assert_eq!((swept.stats.swept, swept.stats.cut_off), (1, 0));
    assert_eq!(
        swept.stock.get(Resource::Reed),
        100 + 5,
        "half the cargo goes"
    );
    assert_eq!(swept.tallies, 100 + 5);
    assert!(events.iter().any(|e| matches!(
        e,
        Event::TripEnded {
            ending: Ending::Swept,
            ..
        }
    )));
    // The same camp on a boat is afloat, not swept (and the water has no boat to keep).
    let (afloat, _) = run(1);
    assert!(afloat.trip.is_some());
    assert_eq!(afloat.stats.swept, 0);
}

#[test]
fn a_party_out_when_the_turns_run_out_is_cut_off_with_half_the_cargo() {
    let fx = Fx::quiet();
    let env = fx.env(DAY);
    let mut clan = fx.clan();
    let ids = party(&clan, 2);
    out(&fx, &mut clan, &ids, kit());
    let away = fx.tile_of(TileKind::Island, &clan);
    {
        let t = clan.trip.as_mut().unwrap();
        t.at = away;
        t.loot.add(Resource::Reed, 11);
    }
    clan.turns_used = 29;
    let events = apply(&env, &mut clan, TripAction::Camp { turns: 1 }, &mut rng(1)).unwrap();
    assert!(clan.trip.is_none());
    assert_eq!(clan.turns_used, 30);
    assert_eq!((clan.stats.cut_off, clan.stats.returned), (1, 0));
    assert_eq!(
        clan.stock.get(Resource::Reed),
        100 + 5,
        "half, rounded against the party"
    );
    assert!(events.iter().any(|e| matches!(
        e,
        Event::TripEnded {
            ending: Ending::Stranded,
            ..
        }
    )));
    // At home with no turns left is fine: nobody is cut off in their own hold.
    let mut safe = fx.clan();
    out(&fx, &mut safe, &ids, kit());
    safe.turns_used = 26;
    assert!(apply(&env, &mut safe, TripAction::Camp { turns: 4 }, &mut rng(1)).is_ok());
    assert!(safe.trip.is_some(), "still at the hold");
}

#[test]
fn a_trip_left_open_past_midnight_is_cut_off_by_the_catch_up() {
    let fx = Fx::quiet();
    let mut clan = fx.clan();
    let ids = party(&clan, 2);
    out(&fx, &mut clan, &ids, kit());
    let away = fx.tile_of(TileKind::Island, &clan);
    {
        let t = clan.trip.as_mut().unwrap();
        t.at = away;
        t.loot.add(Resource::Salt, 9);
    }
    let env = fx.env(DAY + 1);
    let events = crate::engine::catch_up(&env, &mut clan, DAY + 1);
    assert!(clan.trip.is_none());
    assert_eq!(clan.stats.cut_off, 1);
    assert_eq!(clan.stock.get(Resource::Salt), 100 + 4);
    assert!(events.iter().any(|e| matches!(
        e,
        Event::TripEnded {
            ending: Ending::Overnight,
            ..
        }
    )));
    assert!(events.iter().any(|e| matches!(e, Event::Night { .. })));
    assert_eq!(clan.turns_used, 0, "and the new day is fresh");
}

// ------------------------------------------------------------------ gathering

fn gather_total(
    fx: &Fx,
    ids: &[u16],
    tile: (u8, u8),
    kit: Kit,
    seed: u64,
    tweak: impl Fn(&mut Clan),
) -> u32 {
    let env = fx.env(DAY);
    let mut clan = fx.clan();
    tweak(&mut clan);
    depart(&env, &mut clan, ids, kit, None, &mut Vec::new()).unwrap();
    clan.trip.as_mut().unwrap().at = tile;
    apply(&env, &mut clan, TripAction::Gather, &mut rng(seed)).unwrap();
    clan.trip.as_ref().unwrap().loot.counts().iter().sum()
}

#[test]
fn gathering_follows_the_trade_the_ground_the_hands_and_the_day() {
    let fx = Fx::quiet();
    let clan = fx.clan();
    let reeds = fx.tile_of(TileKind::Reedbed, &clan);
    let (wader, raker) = (clan.crew[0].id, clan.crew[1].id);
    let sum = |ids: &[u16], f: &dyn Fn(&mut Clan)| -> u32 {
        (0..60)
            .map(|s| gather_total(&fx, ids, reeds, Kit::default(), s, f))
            .sum()
    };
    let none = |_: &mut Clan| {};
    let by_raker = sum(&[raker], &none);
    let by_wader = sum(&[wader], &none);
    assert!(by_raker > by_wader, "{by_raker} against {by_wader}");
    // Skill and traits change the take.
    let skilled = sum(&[raker], &|c: &mut Clan| {
        c.crew[1].xp[Trade::Raker.index()] = 200;
    });
    assert!(skilled > by_raker, "{skilled} against {by_raker}");
    let lucky = sum(&[raker], &|c: &mut Clan| {
        c.crew[1].traits.push("lucky".into());
    });
    assert!(lucky > by_raker, "lucky hands find a rich seam");
    let injured = sum(&[raker], &|c: &mut Clan| {
        c.crew[1].injuries.push(Injured {
            id: "gash".into(),
            days: 5,
        });
    });
    assert!(injured < by_raker, "a hurt hand brings less");
    // More hands bring more (each is a hand's worth).
    let two = sum(&[raker, wader], &none);
    assert!(two > by_raker);
    // A greedy hand keeps part of it: the party brings in less and the screen says who.
    let env = fx.env(DAY);
    let mut greedy = fx.clan();
    greedy.crew[1].traits.push("greedy".into());
    let mut kept = 0;
    for s in 0..80 {
        let mut c = greedy.clone();
        depart(
            &env,
            &mut c,
            &[raker],
            Kit::default(),
            None,
            &mut Vec::new(),
        )
        .unwrap();
        c.trip.as_mut().unwrap().at = reeds;
        let ev = apply(&env, &mut c, TripAction::Gather, &mut rng(s)).unwrap();
        kept += ev
            .iter()
            .filter(|e| matches!(e, Event::Pocketed { .. }))
            .count();
    }
    assert!(kept > 5, "{kept}");
}

#[test]
fn ground_runs_out_open_water_needs_a_boat_and_the_hold_is_not_a_quarry() {
    let fx = Fx::quiet();
    let env = fx.env(DAY);
    let clan = fx.clan();
    let ids = party(&clan, 2);
    let reeds = fx.tile_of(TileKind::Reedbed, &clan);
    let mut c = clan.clone();
    depart(&env, &mut c, &ids, kit(), None, &mut Vec::new()).unwrap();
    // The hold's own ground is for shifts.
    let err = apply(&env, &mut c, TripAction::Gather, &mut rng(1)).unwrap_err();
    assert!(matches!(err, ActionError::Cannot(_)));
    c.trip.as_mut().unwrap().at = reeds;
    let first = apply(&env, &mut c, TripAction::Gather, &mut rng(1)).unwrap();
    let second = apply(&env, &mut c, TripAction::Gather, &mut rng(1)).unwrap();
    let got = |ev: &[Event]| -> u32 {
        ev.iter()
            .map(|e| match e {
                Event::Found { qty, .. } => *qty,
                _ => 0,
            })
            .sum()
    };
    assert!(got(&first) > got(&second), "the second pass is thin");
    let before = c.clone();
    let err = apply(&env, &mut c, TripAction::Gather, &mut rng(1)).unwrap_err();
    assert!(matches!(err, ActionError::Cannot(_)), "worked out");
    assert_eq!(c, before, "a refusal changes nothing");
    // Open water needs a boat.
    let channel = fx.tile_of(TileKind::Channel, &clan);
    let mut dry = clan.clone();
    depart(&env, &mut dry, &ids, kit(), None, &mut Vec::new()).unwrap();
    dry.trip.as_mut().unwrap().at = channel;
    assert!(matches!(
        apply(&env, &mut dry, TripAction::Gather, &mut rng(1)),
        Err(ActionError::Cannot(_))
    ));
    let mut wet = clan.clone();
    depart(
        &env,
        &mut wet,
        &ids,
        Kit {
            boats: 1,
            ..Kit::default()
        },
        None,
        &mut Vec::new(),
    )
    .unwrap();
    wet.trip.as_mut().unwrap().at = channel;
    apply(&env, &mut wet, TripAction::Gather, &mut rng(1)).unwrap();
    assert!(wet.trip.as_ref().unwrap().loot.get(Resource::Fish) > 0);
    // Ground with nothing on it.
    let mut bare = clan.clone();
    depart(&env, &mut bare, &ids, kit(), None, &mut Vec::new()).unwrap();
    bare.trip.as_mut().unwrap().at = fx.tile_of(TileKind::Wreck, &clan);
    assert!(apply(&env, &mut bare, TripAction::Gather, &mut rng(1)).is_ok());
}

#[test]
fn a_full_pack_leaves_the_rest_behind_and_jettison_lightens_it() {
    let fx = Fx::quiet();
    let env = fx.env(DAY);
    let mut clan = fx.clan();
    let ids = party(&clan, 1);
    out(&fx, &mut clan, &ids, Kit::default());
    let mut trip = clan.trip.take().unwrap();
    let mut events = Vec::new();
    let hard = trip.capacity(&clan, &fx.cat) * 3 / 2;
    add_loot(&env, &clan, &mut trip, Resource::Timber, 100, &mut events);
    assert_eq!(
        trip.loot.get(Resource::Timber) * 2,
        hard / 2 * 2,
        "timber weighs two"
    );
    assert!(trip.load() <= hard);
    assert!(events.iter().any(|e| matches!(e, Event::LeftBehind { .. })));
    assert!(trip.overloaded(&clan, &fx.cat));
    clan.trip = Some(trip);
    let ev = apply(
        &env,
        &mut clan,
        TripAction::Jettison {
            what: Resource::Timber,
            qty: 100,
        },
        &mut rng(1),
    )
    .unwrap();
    assert!(matches!(ev.as_slice(), [Event::Dropped { .. }]));
    assert!(!clan.trip.as_ref().unwrap().overloaded(&clan, &fx.cat));
    assert!(matches!(
        apply(
            &env,
            &mut clan,
            TripAction::Jettison {
                what: Resource::Timber,
                qty: 1
            },
            &mut rng(1)
        ),
        Err(ActionError::Cannot(_))
    ));
}

// ------------------------------------------------------------------ which scene, and who sees what

fn ids_in(fx: &Fx, env: &Env<'_>, clan: &Clan, place: Place) -> Vec<String> {
    let trip = clan.trip.as_ref().unwrap();
    pool(env, clan, trip, place)
        .into_iter()
        .map(|(i, _)| fx.cat.events[i].id.clone())
        .collect()
}

fn on_trip(fx: &Fx, ids: &[u16]) -> Clan {
    let mut clan = fx.clan();
    out(fx, &mut clan, ids, kit());
    clan
}

#[test]
fn scenes_follow_the_tile_the_water_the_hour_and_the_season() {
    let fx = Fx::new();
    let ids = [1, 2];
    let mut clan = on_trip(&fx, &ids);
    // A mudflat scene needs a mudflat, and low or middling water.
    let (day, turn) = fx.moment(|l| l < 40);
    clan.turns_used = turn;
    let env = fx.env(day);
    let mud = ids_in(&fx, &env, &clan, Place::Tile(TileKind::Mudflat));
    assert!(mud.contains(&"silt-sledge".to_owned()), "{mud:?}");
    let reeds = ids_in(&fx, &env, &clan, Place::Tile(TileKind::Reedbed));
    assert!(!reeds.contains(&"silt-sledge".to_owned()));
    assert!(reeds.contains(&"reed-whisper".to_owned()));
    // The flood closes it.
    let (day, turn) = fx.moment(|l| l >= 70);
    clan.turns_used = turn;
    let flood = ids_in(&fx, &fx.env(day), &clan, Place::Tile(TileKind::Mudflat));
    assert!(!flood.contains(&"silt-sledge".to_owned()));
    assert!(flood.contains(&"storm-warning".to_owned()), "{flood:?}");
    // Fog is for the dim hours.
    clan.turns_used = 15;
    assert!(
        !ids_in(&fx, &fx.env(DAY), &clan, Place::Tile(TileKind::Mudflat))
            .contains(&"fog-bank".to_owned()),
        "noon"
    );
    clan.turns_used = 2;
    assert!(
        ids_in(&fx, &fx.env(DAY), &clan, Place::Tile(TileKind::Mudflat))
            .contains(&"fog-bank".to_owned()),
        "the small hours"
    );
    // The season: nests are for the early weeks.
    clan.turns_used = 12;
    let early = ids_in(&fx, &fx.env(DAY), &clan, Place::Tile(TileKind::Reedbed));
    assert!(early.contains(&"egg-island".to_owned()));
    clan.founded = DAY - 30;
    let later = ids_in(&fx, &fx.env(DAY), &clan, Place::Tile(TileKind::Reedbed));
    assert!(!later.contains(&"egg-island".to_owned()));
    // Places and camps have scenes of their own.
    let shrine = ids_in(&fx, &fx.env(DAY), &clan, Place::Poi(PoiKind::Shrine));
    assert!(shrine.contains(&"shrine-offering".to_owned()));
    assert!(!shrine.contains(&"silt-sledge".to_owned()));
    let camp = ids_in(&fx, &fx.env(DAY), &clan, Place::Camp);
    assert!(camp.contains(&"night-watch".to_owned()) && !camp.contains(&"fog-bank".to_owned()));
}

#[test]
fn the_clans_history_decides_what_can_be_drawn() {
    let fx = Fx::new();
    let mut clan = on_trip(&fx, &[1, 2]);
    clan.turns_used = 12;
    let env = fx.env(DAY);
    let tile = Place::Tile(TileKind::Mudflat);
    assert!(!ids_in(&fx, &env, &clan, tile).contains(&"water-remembers".to_owned()));
    clan.add_mark("robbed-the-dead");
    assert!(ids_in(&fx, &env, &clan, tile).contains(&"water-remembers".to_owned()));
    // Putting it right closes the scene again: the unmark effect runs.
    let def = fx.cat.event("water-remembers").unwrap();
    let put_back = &def.options[1].win;
    let mut trip = clan.trip.take().unwrap();
    trip.kit.tools = 1;
    let end = run_effects(
        &env,
        &mut clan,
        &mut trip,
        &put_back.fx,
        &mut rng(1),
        &mut Vec::new(),
    );
    assert_eq!(end, None);
    clan.trip = Some(trip);
    assert!(!clan.has_mark("robbed-the-dead"));
    assert!(!ids_in(&fx, &env, &clan, tile).contains(&"water-remembers".to_owned()));
    // A hermit's gift needs the hermit to have been fed; a lair's hag needs the bell rung.
    let hermit = Place::Poi(PoiKind::Hermit);
    assert!(!ids_in(&fx, &env, &clan, hermit).contains(&"hermit-gift".to_owned()));
    clan.add_mark("fed-the-hermit");
    assert!(ids_in(&fx, &env, &clan, hermit).contains(&"hermit-gift".to_owned()));
    let lair = Place::Poi(PoiKind::Lair);
    assert!(!ids_in(&fx, &env, &clan, lair).contains(&"hag-lair".to_owned()));
    clan.add_mark("rang-the-bell");
    assert!(ids_in(&fx, &env, &clan, lair).contains(&"hag-lair".to_owned()));
    // A deed done in one scene is the cause of another: losing the snare makes the cutters
    // enemies, and the next prints in the mud are theirs.
    let snare = fx.cat.event("cutters-snare").unwrap();
    let lose = snare.options[0].lose.as_ref().unwrap();
    let mut trip = clan.trip.take().unwrap();
    run_effects(
        &env,
        &mut clan,
        &mut trip,
        &lose.fx,
        &mut rng(1),
        &mut Vec::new(),
    );
    clan.trip = Some(trip);
    assert!(clan.has_mark("crossed-the-cutters"));
    assert!(matches!(clan.trip.as_ref().unwrap().phase, Phase::Fight(_)));
    clan.trip.as_mut().unwrap().phase = Phase::Free;
    let tracks = fx.cat.event("rival-tracks").unwrap();
    let trip = clan.trip.as_ref().unwrap();
    assert!(offered_options(&fx.cat, &clan, trip, tracks).contains(&2));
}

#[test]
fn a_rare_scene_is_seen_once_and_a_party_that_suits_it_sees_it_more() {
    let fx = Fx::new();
    let mut clan = on_trip(&fx, &[1, 2]);
    let (day, turn) = fx.moment(|l| l < 35);
    clan.turns_used = turn;
    let env = fx.env(day);
    let tile = Place::Tile(TileKind::Mudflat);
    assert!(ids_in(&fx, &env, &clan, tile).contains(&"silt-whale".to_owned()));
    let scene = Scene {
        event: "silt-whale".into(),
        offered: vec![0, 1],
    };
    let mut trip = clan.trip.take().unwrap();
    raise(&mut clan, &mut trip, &fx.cat, scene, &mut Vec::new());
    clan.trip = Some(trip);
    assert!(clan.has_mark("seen:silt-whale"));
    assert!(!ids_in(&fx, &env, &clan, tile).contains(&"silt-whale".to_owned()));
    // A reed-witch in the party makes the reeds' whispers three times as likely.
    let weight = |clan: &Clan| {
        let trip = clan.trip.as_ref().unwrap();
        let i = fx
            .cat
            .events
            .iter()
            .position(|e| e.id == "reed-whisper")
            .unwrap();
        pool(&env, clan, trip, Place::Tile(TileKind::Reedbed))
            .into_iter()
            .find(|(j, _)| *j == i)
            .map(|(_, w)| w)
            .unwrap()
    };
    let plain = on_trip(&fx, &[1, 2]);
    let witch = on_trip(&fx, &[1, 6]);
    assert_eq!(weight(&witch), 3 * weight(&plain));
}

#[test]
fn the_same_scene_offers_a_different_list_to_a_different_party() {
    let fx = Fx::new();
    let mud = fx.cat.event("sucking-mud").unwrap();
    let cat = &fx.cat;
    // Wader and raker with rope: all three ways across.
    let a = on_trip(&fx, &[1, 2]);
    assert_eq!(
        offered_options(cat, &a, a.trip.as_ref().unwrap(), mud),
        vec![0, 1, 2]
    );
    // Raker and fighter with no rope: only the long way round.
    let mut b = fx.clan();
    out(&fx, &mut b, &[2, 4], Kit::default());
    assert_eq!(
        offered_options(cat, &b, b.trip.as_ref().unwrap(), mud),
        vec![0]
    );
    // One wader alone with rope: the wader's way, but not the rope team (that wants two).
    let mut c = fx.clan();
    out(&fx, &mut c, &[1], kit());
    assert_eq!(
        offered_options(cat, &c, c.trip.as_ref().unwrap(), mud),
        vec![0, 1]
    );
    // The healer and the reed-witch see what no one else does.
    let castaway = fx.cat.event("castaway").unwrap();
    let mut with = fx.clan();
    out(&fx, &mut with, &[1, 5], kit());
    let mut without = fx.clan();
    out(&fx, &mut without, &[1, 2], kit());
    let list = |c: &Clan| offered_options(cat, c, c.trip.as_ref().unwrap(), castaway);
    assert_eq!(list(&with), vec![0, 1, 2]);
    assert_eq!(list(&without), vec![0, 2]);
}

#[test]
fn a_scar_closes_one_door_and_opens_another() {
    let fx = Fx::new();
    let cat = &fx.cat;
    let storm = cat.event("storm-warning").unwrap();
    let hum = cat.event("deep-hum").unwrap();
    let list = |c: &Clan, e| offered_options(cat, c, c.trip.as_ref().unwrap(), e);
    let sound = on_trip(&fx, &[1, 2]);
    assert!(
        list(&sound, storm).contains(&2),
        "a sound party can run for it"
    );
    assert!(!list(&sound, hum).contains(&1), "and cannot hear the Deep");
    let mut scarred = fx.clan();
    scarred.crew[1].traits.push("limp".into());
    out(&fx, &mut scarred, &[1, 2], kit());
    assert!(
        !list(&scarred, storm).contains(&2),
        "a limp rules the run out"
    );
    assert!(
        list(&scarred, hum).contains(&1),
        "and the scarred hear the hum"
    );
    // A scar earned in a fight is as good as a hired one: injury, healing, scar.
    let mut c = fx.clan();
    out(&fx, &mut c, &[1, 2], kit());
    let mut scar_seen = false;
    for seed in 0..400 {
        let mut x = c.clone();
        let mut ev = Vec::new();
        x.crew[1].injuries.push(Injured {
            id: "hamstrung".into(),
            days: 1,
        });
        finish_injury(&fx.cat, &mut x, 2, "hamstrung", &mut rng(seed), &mut ev);
        if x.crew[1].traits.iter().any(|t| t == "limp") {
            scar_seen = true;
            let t = x.trip.as_ref().unwrap();
            assert!(!offered_options(cat, &x, t, storm).contains(&2));
            break;
        }
    }
    assert!(scar_seen, "a hamstrung leg sometimes heals into a limp");
}

#[test]
fn skill_luck_and_a_steady_hand_move_the_odds_and_a_bond_opens_a_way() {
    let fx = Fx::new();
    let cat = &fx.cat;
    let odds =
        |c: &Clan, ev: &str, opt| fail_chance(cat, c, c.trip.as_ref().unwrap(), ev, opt).unwrap();
    let mut green = fx.clan();
    out(&fx, &mut green, &[1, 2], kit());
    let mut skilled = fx.clan();
    skilled.crew[0].xp[Trade::Wader.index()] = 36;
    out(&fx, &mut skilled, &[1, 2], kit());
    // The cutters' snare: 45 base, fighter skill.
    assert_eq!(odds(&green, "sucking-mud", 1), 30 - 8);
    assert_eq!(odds(&skilled, "sucking-mud", 1), 30 - 8 * 3);
    let mut lucky = fx.clan();
    lucky.crew[0].traits.push("lucky".into());
    out(&fx, &mut lucky, &[1, 2], kit());
    assert!(odds(&lucky, "sucking-mud", 1) < odds(&green, "sucking-mud", 1));
    let mut clumsy = fx.clan();
    clumsy.crew[1].traits.push("clumsy".into());
    out(&fx, &mut clumsy, &[1, 2], kit());
    assert!(odds(&clumsy, "sucking-mud", 1) > odds(&green, "sucking-mud", 1));
    // An option with no risk never goes badly; the floor and ceiling hold.
    assert_eq!(odds(&green, "sucking-mud", 0), 0);
    let mut master = fx.clan();
    master.crew[0].xp[Trade::Wader.index()] = 400;
    out(&fx, &mut master, &[1, 2], kit());
    assert_eq!(odds(&master, "sucking-mud", 1), 3);
    // Skill used is skill learned, win or lose.
    let env = fx.env(DAY);
    let mut c = fx.clan();
    out(&fx, &mut c, &[1, 2], kit());
    let before = c.crew[0].xp[Trade::Wader.index()];
    c.trip.as_mut().unwrap().phase = Phase::Scene(Scene {
        event: "sucking-mud".into(),
        offered: vec![0, 1, 2],
    });
    apply(&env, &mut c, TripAction::Choose(1), &mut rng(5)).unwrap();
    assert!(c.crew[0].xp[Trade::Wader.index()] > before);
    // A bond opens the ferryman's fair coin.
    let ferry = cat.event("the-old-ferryman").unwrap();
    let mut bonded = fx.clan();
    bonded.share_hardship(1, 2, 8);
    out(&fx, &mut bonded, &[1, 2], kit());
    let strangers = on_trip(&fx, &[1, 2]);
    let list = |c: &Clan| offered_options(cat, c, c.trip.as_ref().unwrap(), ferry);
    assert_eq!(list(&bonded), vec![0, 1, 2]);
    assert_eq!(list(&strangers), vec![0, 2]);
}

#[test]
fn a_steady_hand_is_hurt_less_often_than_a_clumsy_one() {
    let fx = Fx::new();
    let env = fx.env(DAY);
    let hurt_in = |trait_id: &str| -> u32 {
        (0..300)
            .map(|s| {
                let mut c = fx.clan();
                c.crew[0].traits.push(trait_id.to_owned());
                out(&fx, &mut c, &[1], kit());
                let mut trip = c.trip.take().unwrap();
                let mut ev = Vec::new();
                run_effects(
                    &env,
                    &mut c,
                    &mut trip,
                    &[Effect::Hurt(Who::Lead, Cause::Hurt)],
                    &mut rng(s),
                    &mut ev,
                );
                u32::from(ev.iter().any(|e| matches!(e, Event::Injured { .. })))
            })
            .sum()
    };
    let (steady, clumsy) = (hurt_in("steady"), hurt_in("clumsy"));
    assert_eq!(clumsy, 300, "a clumsy hand is always caught");
    assert!(
        steady < 220 && steady > 120,
        "steady hands are caught about 60%: {steady}"
    );
}

// ------------------------------------------------------------------ no dead content

/// Build a clan and a trip in which `needs` all hold, as far as a requirement can be met.
fn satisfy(fx: &Fx, needs: &[&Cond]) -> Clan {
    let mut clan = fx.clan();
    let ids = party(&clan, 5);
    out(fx, &mut clan, &ids, Kit::default());
    let mut next = 0usize;
    let mut trip = clan.trip.take().unwrap();
    for cond in needs.iter().filter(|c| !c.not) {
        match &cond.req {
            Req::Trade(t, l) => {
                let c = &mut clan.crew[next % 5];
                next += 1;
                c.trade = *t;
                c.xp[t.index()] = 4 * u16::from(*l) * u16::from(*l);
            }
            Req::Skill(t, l) => {
                let c = &mut clan.crew[next % 5];
                next += 1;
                c.xp[t.index()] = 4 * u16::from(*l) * u16::from(*l);
            }
            Req::Trait(id) => {
                clan.crew[next % 5].traits.push(id.clone());
                next += 1;
            }
            Req::Scar => clan.crew[next % 5].traits.push("limp".into()),
            Req::Bond => {
                clan.share_hardship(ids[0], ids[1], 8);
            }
            Req::Mark(m) => clan.add_mark(m),
            Req::Carry(r, n) => {
                let n = *n;
                match r {
                    Resource::Food => trip.kit.food = u8::try_from(n).unwrap(),
                    Resource::Rope => trip.kit.rope = u8::try_from(n).unwrap(),
                    Resource::Peat => trip.kit.peat = u8::try_from(n).unwrap(),
                    Resource::Tools => trip.kit.tools = u8::try_from(n).unwrap(),
                    other => trip.loot.add(*other, n),
                }
            }
            Req::Size(_) => {}
            Req::Boat => trip.kit.boats = 1,
        }
    }
    clan.trip = Some(trip);
    clan
}

/// The place, the day and the hour at which `e` can be drawn for a clan.
fn setting<'a>(fx: &'a Fx, e: &crate::lore::EventDef, clan: &mut Clan) -> (Env<'a>, Place) {
    let place = if let Some(k) = e.poi.first() {
        Place::Poi(*k)
    } else if e.camp {
        Place::Camp
    } else {
        Place::Tile(e.tiles.first().copied().unwrap_or(TileKind::Mudflat))
    };
    let age = match e.stage.first() {
        Some(Stage::Mid) => 20,
        Some(Stage::Late) => 40,
        _ => 0,
    };
    for day in DAY + age..DAY + age + 14 {
        for turn in 0..26u8 {
            let minute = Tide::turn_minute(day, turn, 30);
            let water = Water::of(fx.tide.level(minute));
            let part = Daypart::of(minute);
            if (e.water.is_empty() || e.water.contains(&water))
                && (e.day.is_empty() || e.day.contains(&part))
            {
                clan.founded = day - age;
                clan.turns_used = turn;
                return (fx.env(day), place);
            }
        }
    }
    panic!("`{}` can never be drawn: no hour fits", e.id);
}

#[test]
fn every_scene_and_every_option_can_be_reached() {
    let fx = Fx::new();
    for e in &fx.cat.events {
        let scene_needs: Vec<&Cond> = e.needs.iter().collect();
        let mut clan = satisfy(&fx, &scene_needs);
        let (env, place) = setting(&fx, e, &mut clan);
        let here = ids_in(&fx, &env, &clan, place);
        assert!(here.contains(&e.id), "`{}` is never drawn: {here:?}", e.id);
        for (i, o) in e.options.iter().enumerate() {
            let needs: Vec<&Cond> = e.needs.iter().chain(&o.needs).collect();
            let c = satisfy(&fx, &needs);
            let offered = offered_options(&fx.cat, &c, c.trip.as_ref().unwrap(), e);
            assert!(
                offered.contains(&u8::try_from(i).unwrap()),
                "`{}` option {} can never be offered ({offered:?})",
                e.id,
                i + 1
            );
        }
    }
}

#[test]
fn every_outcome_runs_cleanly_and_keeps_the_pack_within_its_limits() {
    let fx = Fx::new();
    let mut effects_seen = std::collections::HashSet::new();
    for e in &fx.cat.events {
        for (i, o) in e.options.iter().enumerate() {
            let needs: Vec<&Cond> = e.needs.iter().chain(&o.needs).collect();
            for outcome in std::iter::once(&o.win).chain(o.lose.as_ref()) {
                for fx_item in &outcome.fx {
                    effects_seen.insert(std::mem::discriminant(fx_item));
                }
                for seed in 0..6 {
                    let mut clan = satisfy(&fx, &needs);
                    let env = fx.env(DAY);
                    let mut trip = clan.trip.take().unwrap();
                    let hard = trip.capacity(&clan, &fx.cat) * 3 / 2;
                    let start = trip.load();
                    let mut ev = Vec::new();
                    let end = run_effects(
                        &env,
                        &mut clan,
                        &mut trip,
                        &outcome.fx,
                        &mut rng(seed),
                        &mut ev,
                    );
                    assert!(
                        trip.load() <= hard.max(start) + 6,
                        "`{}` option {} overfills the pack",
                        e.id,
                        i + 1
                    );
                    assert!(trip.party.iter().all(|id| clan.crew_by(*id).is_some()));
                    assert_eq!(end.is_some(), trip.party.is_empty());
                    if let Phase::Fight(f) = &trip.phase {
                        assert!(f.units.iter().any(|u| u.side == Side::Foe));
                    }
                }
            }
        }
    }
    assert_eq!(
        effects_seen.len(),
        16,
        "every kind of effect is used by some scene"
    );
}

// ------------------------------------------------------------------ fights on a trip

fn fighters(fx: &Fx, n: usize, arms: u8) -> Clan {
    let mut clan = fx.clan();
    for c in clan.crew.iter_mut().take(n) {
        c.trade = Trade::Fighter;
        c.xp[Trade::Fighter.index()] = 36;
        c.attrs = [7, 5, 6];
    }
    let ids = party(&clan, n);
    out(
        fx,
        &mut clan,
        &ids,
        Kit {
            food: 4,
            arms,
            ..Kit::default()
        },
    );
    clan
}

#[test]
fn a_won_fight_pays_the_drops_hurts_the_fallen_and_clears_a_lair() {
    let fx = Fx::quiet();
    let (day, turn) = fx.moment(|l| l < 30);
    let env = fx.env(day);
    let lair = fx
        .map
        .pois
        .iter()
        .find(|p| p.kind == PoiKind::Lair)
        .map(|p| (p.x, p.y))
        .unwrap();
    let mut wins = 0;
    let mut hurt = 0;
    for seed in 0..40 {
        let mut clan = fighters(&fx, 2, 1);
        clan.turns_used = turn;
        clan.trip.as_mut().unwrap().at = lair;
        // A boat keeps the party off the bottom while it works, whatever the tide does.
        clan.trip.as_mut().unwrap().kit.boats = 1;
        let mut trip = clan.trip.take().unwrap();
        let mut ev = Vec::new();
        let mut r = rng(seed);
        let end = begin_fight(&env, &mut clan, &mut trip, "hound-pack", &mut r, &mut ev);
        assert_eq!(end, None);
        clan.trip = Some(trip);
        let out_ev = apply(&env, &mut clan, TripAction::Quick, &mut r).unwrap();
        if out_ev.iter().any(|e| {
            matches!(
                e,
                Event::Skirmish {
                    verdict: Verdict::Won
                }
            )
        }) {
            wins += 1;
            let t = clan
                .trip
                .as_ref()
                .unwrap_or_else(|| panic!("seed {seed} turn {turn}: {out_ev:?}"));
            assert_eq!(
                t.loot.get(Resource::Peat),
                9,
                "three hounds, three peat each"
            );
            assert!(clan.has_mark(&format!("done:{},{}", lair.0, lair.1)));
            assert_eq!(clan.stats.fights_won, 1);
            assert!(matches!(t.phase, Phase::Free));
            hurt +=
                u32::try_from(clan.crew.iter().filter(|c| !c.injuries.is_empty()).count()).unwrap();
        }
    }
    assert!(wins >= 20, "two fighters usually beat three hounds: {wins}");
    assert!(hurt > 0, "and some of them pay for it");
}

#[test]
fn a_beaten_party_is_routed_and_the_dead_are_remembered() {
    let fx = Fx::quiet();
    let env = fx.env(DAY);
    let mut routed = 0;
    let mut dead = 0;
    for seed in 0..60 {
        let mut clan = fx.clan();
        // One raker against the warden.
        out(&fx, &mut clan, &[2], Kit::default());
        clan.trip.as_mut().unwrap().loot.add(Resource::Reed, 8);
        let mut trip = clan.trip.take().unwrap();
        let mut r = rng(seed);
        let end = begin_fight(
            &env,
            &mut clan,
            &mut trip,
            "warden",
            &mut r,
            &mut Vec::new(),
        );
        assert_eq!(end, None);
        clan.trip = Some(trip);
        // The party only braces (a quick-resolve would run for it): the warden wins.
        let mut events = Vec::new();
        for _ in 0..30 {
            if clan.trip.is_none() {
                break;
            }
            events.extend(
                apply(
                    &env,
                    &mut clan,
                    TripAction::Fight(FightAction::Brace),
                    &mut r,
                )
                .unwrap(),
            );
        }
        let ended = events.iter().find_map(|e| match e {
            Event::TripEnded { ending, .. } => Some(*ending),
            _ => None,
        });
        match ended {
            Some(Ending::Routed) => {
                routed += 1;
                assert_eq!(clan.stock.get(Resource::Reed), 100, "the cargo is gone");
                assert_eq!(clan.crew.len(), 6, "the survivor limps home");
            }
            Some(Ending::Lost) => {
                dead += 1;
                assert_eq!(clan.crew.len(), 5);
                assert_eq!(clan.hall.len(), 1, "the hall remembers");
                assert_eq!(clan.stats.lost_parties, 1);
            }
            Some(Ending::Stranded | Ending::Swept) => {}
            other => panic!("{other:?}: {events:?}"),
        }
    }
    assert!(routed > 10 && dead > 5, "{routed} routed, {dead} dead");
}

#[test]
fn fleeing_costs_a_quarter_of_the_cargo_and_always_works_in_the_end() {
    let fx = Fx::quiet();
    let env = fx.env(DAY);
    let mut clan = fighters(&fx, 1, 0);
    clan.turns_used = 12;
    clan.trip.as_mut().unwrap().loot.add(Resource::Reed, 12);
    clan.trip.as_mut().unwrap().tallies = 10;
    let mut trip = clan.trip.take().unwrap();
    let mut r = rng(7);
    begin_fight(
        &env,
        &mut clan,
        &mut trip,
        "wight-and-hounds",
        &mut r,
        &mut Vec::new(),
    );
    clan.trip = Some(trip);
    let mut fled = false;
    for _ in 0..30 {
        let Some(t) = clan.trip.as_ref() else { break };
        if !matches!(t.phase, Phase::Fight(_)) {
            fled = true;
            break;
        }
        let _ = apply(
            &env,
            &mut clan,
            TripAction::Fight(FightAction::Flee),
            &mut r,
        );
    }
    assert!(fled, "retreat is always on offer");
    if let Some(t) = &clan.trip {
        assert_eq!(t.loot.get(Resource::Reed), 9);
        assert_eq!(t.tallies, 7);
    }
}

#[test]
fn a_fight_you_cannot_do_that_in_changes_nothing() {
    let fx = Fx::quiet();
    let env = fx.env(DAY);
    let mut clan = fx.clan();
    // Healer and reed-witch stand in the back: they cannot strike.
    out(&fx, &mut clan, &[5, 6, 4], Kit::default());
    clan.turns_used = 10;
    let mut trip = clan.trip.take().unwrap();
    let mut r = rng(3);
    begin_fight(
        &env,
        &mut clan,
        &mut trip,
        "hound-pack",
        &mut r,
        &mut Vec::new(),
    );
    clan.trip = Some(trip);
    let before = clan.clone();
    let Phase::Fight(f) = &clan.trip.as_ref().unwrap().phase else {
        panic!("fight");
    };
    let me = usize::from(f.order[usize::from(f.cursor)]);
    let foe = f.alive(Side::Foe).next().unwrap();
    // Whoever is up, the apothecary in the back cannot be asked to do the impossible: a mend
    // on an unhurt ally, a strike at nobody, a special with no target.
    let nobody = u8::try_from(f.units.len() + 3).unwrap();
    for bad in [
        FightAction::Strike { target: nobody },
        FightAction::Mend {
            ally: u8::try_from(foe).unwrap(),
        },
        FightAction::Special {
            target: Some(nobody),
        },
    ] {
        let err = apply(&env, &mut clan, TripAction::Fight(bad), &mut r).unwrap_err();
        assert!(matches!(err, ActionError::Cannot(_)), "{bad:?}: {err:?}");
        assert_eq!(clan, before, "{bad:?}");
    }
    let _ = me;
    // Moving or gathering mid-fight is not allowed either.
    assert!(matches!(
        apply(&env, &mut clan, TripAction::Gather, &mut r),
        Err(ActionError::NotNow(_))
    ));
}

// ------------------------------------------------------------------ the save

#[test]
fn a_trip_in_a_scene_or_a_fight_survives_the_save() {
    let fx = Fx::new();
    let env = fx.env(DAY);
    let mut clan = on_trip(&fx, &[1, 2, 4]);
    clan.add_mark("fed-the-hermit");
    clan.trip.as_mut().unwrap().visited.push((3, 4));
    clan.trip.as_mut().unwrap().worked.push((3, 4, 1));
    clan.trip.as_mut().unwrap().ruin = Some(Ruin { room: 2, rooms: 5 });
    clan.trip.as_mut().unwrap().target = Some((20, 30));
    clan.trip.as_mut().unwrap().loot.add(Resource::Iron, 3);
    let round_trip = |c: &Clan| Clan::decode(&c.encode()).unwrap();
    assert_eq!(round_trip(&clan), clan, "free");
    clan.trip.as_mut().unwrap().phase = Phase::Scene(Scene {
        event: "sucking-mud".into(),
        offered: vec![0, 2],
    });
    assert_eq!(round_trip(&clan), clan, "mid-scene");
    clan.trip.as_mut().unwrap().phase = Phase::Free;
    let mut trip = clan.trip.take().unwrap();
    begin_fight(
        &env,
        &mut clan,
        &mut trip,
        "cutters-road",
        &mut rng(4),
        &mut Vec::new(),
    );
    clan.trip = Some(trip);
    assert!(matches!(clan.trip.as_ref().unwrap().phase, Phase::Fight(_)));
    assert_eq!(round_trip(&clan), clan, "mid-fight");
    // Damage anywhere in the trip's bytes is refused, not guessed at.
    let bytes = clan.encode();
    for cut in [1, 2, 7, 40] {
        assert_eq!(Clan::decode(&bytes[..bytes.len() - cut]), None, "{cut}");
    }
    assert!(bytes.len() < 8_000, "{}", bytes.len());
}

#[test]
fn marks_are_remembered_forgotten_and_capped() {
    let fx = Fx::new();
    let mut clan = fx.clan();
    clan.add_mark("a");
    clan.add_mark("a");
    assert_eq!(clan.marks, vec!["a"]);
    clan.remove_mark("a");
    assert_eq!(clan.marks, Vec::<String>::new());
    for i in 0..(MARKS_CAP + 5) {
        clan.add_mark(&format!("m{i}"));
    }
    assert_eq!(clan.marks.len(), MARKS_CAP);
    assert!(!clan.has_mark("m0") && clan.has_mark(&format!("m{}", MARKS_CAP + 4)));
}

#[test]
fn the_dice_are_in_the_save_so_a_lost_key_cannot_be_rolled_again() {
    let fx = Fx::new();
    let env = fx.env(DAY);
    let mut clan = on_trip(&fx, &[1, 2]);
    clan.trip.as_mut().unwrap().phase = Phase::Scene(Scene {
        event: "sucking-mud".into(),
        offered: vec![0, 1, 2],
    });
    // The same save and the same key give the same result however often it is tried...
    let run = || {
        let mut c = clan.clone();
        let ev = super::apply(&env, &mut c, TripAction::Choose(1)).unwrap();
        (c, ev)
    };
    let first = run();
    for _ in 0..5 {
        assert_eq!(run(), first);
    }
    // ...and the dice moved on, so the next choice is not the same roll again.
    assert_ne!(
        first.0.trip.as_ref().map(|t| t.rng),
        clan.trip.as_ref().map(|t| t.rng)
    );
    // Different dice give different outcomes: the choice goes both ways.
    let mut outcomes = std::collections::BTreeSet::new();
    for seed in 0..40 {
        let mut c = clan.clone();
        c.trip.as_mut().unwrap().rng = DoorRng::from_seed(seed).state();
        let ev = super::apply(&env, &mut c, TripAction::Choose(1)).unwrap();
        outcomes.insert(ev.len() * 100 + usize::try_from(c.stats.injuries).unwrap());
    }
    assert!(outcomes.len() > 1, "{outcomes:?}");
    // A refused action rolls nothing.
    let before = clan.clone();
    let mut again = clan.clone();
    assert!(super::apply(&env, &mut again, TripAction::Gather).is_err());
    assert_eq!(again, before);
    // The session's seed starts the dice once, at departure.
    let dice = |seed: u64| {
        let mut c = fx.clan();
        super::depart(
            &env,
            &mut c,
            &[1, 2],
            kit(),
            None,
            &mut rng(seed),
            &mut Vec::new(),
        )
        .unwrap();
        c.trip.unwrap().rng
    };
    assert_ne!(dice(1), dice(2));
    assert_ne!(dice(1), [0; 4]);
}

#[test]
fn a_scene_the_tables_no_longer_have_passes_and_the_party_goes_on() {
    let fx = Fx::new();
    let env = fx.env(DAY);
    let at = |scene: Scene| {
        let mut clan = on_trip(&fx, &[1, 2]);
        clan.trip.as_mut().unwrap().phase = Phase::Scene(scene);
        clan
    };
    // The scene is gone from the tables.
    let mut gone = at(Scene {
        event: "a-scene-that-was-cut".into(),
        offered: vec![0, 1],
    });
    let ev = super::apply(&env, &mut gone, TripAction::Choose(0)).unwrap();
    assert!(matches!(gone.trip.as_ref().unwrap().phase, Phase::Free));
    assert!(
        ev.iter()
            .any(|e| matches!(e, Event::Told(t) if t.contains("passes")))
    );
    // An option the scene no longer has is a plain refusal that changes nothing.
    let mut cut = at(Scene {
        event: "sucking-mud".into(),
        offered: vec![9],
    });
    let before = cut.clone();
    assert_eq!(
        super::apply(&env, &mut cut, TripAction::Choose(0)),
        Err(ActionError::BadChoice(0))
    );
    assert_eq!(cut, before);
    // A scene with nothing to offer can be let go.
    let mut empty = at(Scene {
        event: "sucking-mud".into(),
        offered: vec![],
    });
    assert!(super::apply(&env, &mut empty, TripAction::Choose(0)).is_ok());
    assert!(matches!(empty.trip.as_ref().unwrap().phase, Phase::Free));
}

#[test]
fn standing_in_the_deep_is_remembered_and_other_ground_is_not() {
    let f = Fx::quiet();
    let mut clan = f.clan();
    for kind in [TileKind::Mudflat, TileKind::Reedbed, TileKind::DrownedTown] {
        super::remember_ground(&mut clan, kind);
        assert!(!clan.social.deep, "{kind:?}");
    }
    super::remember_ground(&mut clan, TileKind::Deep);
    assert!(clan.social.deep);
    // It counts for renown and survives a save.
    let again = Clan::decode(&clan.encode()).unwrap();
    assert!(again.social.deep);
}
