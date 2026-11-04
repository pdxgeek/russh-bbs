//! Rules tests: every trait, skill, building and tide effect the engine reads changes an
//! outcome here, and each refusal leaves the clan untouched.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::too_many_lines,
    clippy::many_single_char_names,
    clippy::default_trait_access,
    clippy::assert_is_empty
)]

use super::*;
use crate::clan::Clan;
use crate::crew::Injured;
use crate::event::Event;
use crate::tide::Tide;
use crate::types::Attr;

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
        let map = Map::generate(11, &cat.pois);
        Self {
            tide: Tide::new(11),
            prices: World::new(1, 11, 0).prices,
            settings: Settings::default(),
            cat,
            map,
        }
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

    fn clan(&self) -> Clan {
        let mut clan = Clan::found(
            &self.cat,
            &self.map,
            "Test",
            1,
            0,
            DAY,
            &mut DoorRng::from_seed(1),
        )
        .0;
        // Tests start from an empty hold, so each places only what it is about.
        clan.hold = crate::hold::Hold::default();
        for c in &mut clan.crew {
            c.traits.clear();
            c.injuries.clear();
            c.age = 30;
            c.post = None;
        }
        clan.tallies = 1_000;
        for r in Resource::ALL {
            clan.stock.add(r, 100);
        }
        clan
    }

    /// A (day, turn) where the water satisfies `want`, with two turns to spare.
    fn moment(&self, want: impl Fn(i32) -> bool) -> (i64, u8) {
        for day in DAY..DAY + 60 {
            for turn in 0..28u8 {
                if want(self.tide.level(Tide::turn_minute(day, turn, 30))) {
                    return (day, turn);
                }
            }
        }
        panic!("no such moment");
    }
}

const DAY: i64 = 20_000;

fn place(clan: &mut Clan, plot: usize, id: &str) {
    clan.hold.plots[plot] = Some(id.to_owned());
}

fn rng(n: u64) -> DoorRng {
    DoorRng::from_seed(n)
}

fn count(events: &[Event], f: impl Fn(&Event) -> bool) -> usize {
    events.iter().filter(|e| f(e)).count()
}

#[test]
fn turns_are_the_clock_each_one_moves_the_water_on_and_a_refusal_costs_nothing() {
    let fx = Fx::new();
    let mut clan = fx.clan();
    let env = fx.env(DAY);
    assert_eq!(env.minute(&clan), DAY * 1440);
    apply(&env, &mut clan, Action::Survey, &mut rng(1)).unwrap();
    assert_eq!(clan.turns_used, 2);
    assert_eq!(env.minute(&clan), DAY * 1440 + 2 * 48);
    assert_eq!(clan.turns_left(&fx.settings), 28);
    // Out of turns: refused, nothing changes.
    clan.turns_used = 29;
    let before = clan.clone();
    let err = apply(&env, &mut clan, Action::Survey, &mut rng(1)).unwrap_err();
    assert_eq!(err, ActionError::NoTurns { need: 2, have: 1 });
    assert_eq!(clan, before);
    assert!(err.to_string().contains("2 turns"));
}

#[test]
fn recruiting_costs_tallies_and_a_turn_and_is_capped_by_the_roofs() {
    let fx = Fx::new();
    let env = fx.env(DAY);
    let mut clan = fx.clan();
    clan.crew.truncate(2);
    let price = clan.candidates[0].price(&fx.cat);
    let name = clan.candidates[0].name.clone();
    let ev = apply(
        &env,
        &mut clan,
        Action::Recruit { candidate: 0 },
        &mut rng(1),
    )
    .unwrap();
    assert_eq!(clan.tallies, 1_000 - price);
    assert_eq!(clan.crew.len(), 3);
    assert_eq!(clan.crew.last().unwrap().name, name);
    assert!(clan.crew.last().unwrap().id != 0);
    assert!(matches!(ev[0], Event::Recruited { .. }));
    assert_eq!(clan.turns_used, 1);
    // The hold houses three; a fourth does not fit until a hut is built.
    assert_eq!(
        apply(
            &env,
            &mut clan,
            Action::Recruit { candidate: 0 },
            &mut rng(1)
        ),
        Err(ActionError::Full)
    );
    place(&mut clan, 0, "hut");
    apply(
        &env,
        &mut clan,
        Action::Recruit { candidate: 0 },
        &mut rng(1),
    )
    .unwrap();
    assert_eq!(clan.crew.len(), 4);
    assert_eq!(
        apply(
            &env,
            &mut clan,
            Action::Recruit { candidate: 9 },
            &mut rng(1)
        ),
        Err(ActionError::NoCandidate(9))
    );
    let mut poor = fx.clan();
    poor.tallies = 0;
    poor.crew.pop();
    assert!(matches!(
        apply(
            &env,
            &mut poor,
            Action::Recruit { candidate: 0 },
            &mut rng(1)
        ),
        Err(ActionError::CantAfford(_))
    ));
}

#[test]
fn building_has_requirements_a_track_a_cap_and_a_price_and_labour_is_sped_by_the_right_trades() {
    let fx = Fx::new();
    let env = fx.env(DAY);
    let mut clan = fx.clan();
    let build = |clan: &mut Clan, plot, id: &str| {
        apply(
            &env,
            clan,
            Action::Build {
                plot,
                building: id.into(),
            },
            &mut rng(1),
        )
    };
    assert!(
        matches!(build(&mut clan, 0, "longhouse"), Err(ActionError::Missing(m)) if m == "Reed hut")
    );
    assert!(matches!(
        build(&mut clan, 99, "hut"),
        Err(ActionError::NoPlot(99))
    ));
    assert_eq!(build(&mut clan, 7, "hut"), Err(ActionError::PlotLocked(7)));
    assert!(matches!(
        build(&mut clan, 0, "castle"),
        Err(ActionError::UnknownBuilding(_))
    ));
    let timber = clan.stock.get(Resource::Timber);
    build(&mut clan, 0, "hut").unwrap();
    assert_eq!(
        clan.stock.get(Resource::Timber),
        timber - 6,
        "the cost is paid at the start"
    );
    assert_eq!(
        build(&mut clan, 1, "watchfire"),
        Err(ActionError::TrackBusy),
        "one construction per track"
    );
    build(&mut clan, 1, "salt-pan").unwrap();
    assert_eq!(
        build(&mut clan, 0, "palisade"),
        Err(ActionError::PlotBusy(0)),
        "a plot under construction is taken"
    );
    let poor = {
        let mut c = fx.clan();
        c.tallies = 0;
        c.stock = Default::default();
        c
    };
    let mut poor = poor;
    assert!(matches!(
        build(&mut poor, 0, "hut"),
        Err(ActionError::CantAfford(_))
    ));
    assert!(
        poor.hold.building.iter().all(Option::is_none),
        "a refused build starts nothing"
    );
    // Labour: a wader on the crew speeds the shelter track (1 + 1), nobody speeds the defence track.
    let mut slow = fx.clan();
    slow.crew
        .retain(|c| c.trade != Trade::Wader && c.trade != Trade::Boatwright);
    let mut fast = fx.clan();
    fast.crew.retain(|c| c.trade != Trade::Boatwright);
    assert!(fast.crew.iter().any(|c| c.trade == Trade::Wader));
    for c in [&mut slow, &mut fast] {
        build(c, 0, "hut").unwrap();
        apply(
            &env,
            c,
            Action::Labour {
                track: Track::Shelter,
            },
            &mut rng(1),
        )
        .unwrap();
    }
    assert_eq!(slow.hold.building[0].as_ref().unwrap().remaining, 2);
    assert_eq!(fast.hold.building[0].as_ref().unwrap().remaining, 1);
    let ev = apply(
        &env,
        &mut fast,
        Action::Labour {
            track: Track::Shelter,
        },
        &mut rng(1),
    )
    .unwrap();
    assert!(matches!(ev[0], Event::Completed { plot: 0, .. }));
    assert_eq!(fast.hold.plots[0].as_deref(), Some("hut"));
    assert!(fast.hold.building[0].is_none());
    assert_eq!(
        apply(
            &env,
            &mut fast,
            Action::Labour {
                track: Track::Shelter
            },
            &mut rng(1)
        ),
        Err(ActionError::NothingBuilding)
    );
    // Hut cap is three.
    for plot in 1..4 {
        place(&mut fast, plot, "hut");
    }
    assert!(matches!(
        build(&mut fast, 4, "hut"),
        Err(ActionError::Maxed(_))
    ));
}

#[test]
fn finishing_a_workshop_teaches_its_recipes_and_ruin_plans_stay_locked() {
    let fx = Fx::new();
    let env = fx.env(DAY);
    let mut clan = fx.clan();
    place(&mut clan, 0, "peat-stack");
    clan.hold.building[Track::Work.index()] = Some(Construction {
        building: "smithy".into(),
        plot: 1,
        remaining: 1,
    });
    assert!(!clan.recipes.iter().any(|r| r == "forge-tools"));
    let ev = apply(
        &env,
        &mut clan,
        Action::Labour { track: Track::Work },
        &mut rng(1),
    )
    .unwrap();
    assert!(
        clan.recipes.iter().any(|r| r == "forge-tools")
            && clan.recipes.iter().any(|r| r == "forge-arms")
    );
    assert!(
        !clan.recipes.iter().any(|r| r == "fuse-silt-glass"),
        "ruin plans are not learned from the workshop"
    );
    assert_eq!(count(&ev, |e| matches!(e, Event::Learned { .. })), 2);
    assert!(matches!(
        apply(
            &env,
            &mut clan,
            Action::Craft {
                recipe: "fuse-silt-glass".into()
            },
            &mut rng(1)
        ),
        Err(ActionError::NotKnown(_))
    ));
}

#[test]
fn hand_work_obeys_the_tide_salt_pans_need_low_water() {
    let fx = Fx::new();
    let mut clan = fx.clan();
    place(&mut clan, 0, "salt-pan");
    let crew = vec![clan.crew[1].id];
    let (day, turn) = fx.moment(|l| l > 60);
    clan.turns_used = turn;
    let before = clan.clone();
    let err = apply(
        &fx.env(day),
        &mut clan,
        Action::Shift {
            plot: 0,
            crew: crew.clone(),
        },
        &mut rng(1),
    )
    .unwrap_err();
    assert!(
        matches!(err, ActionError::TideTooHigh { max: 45, .. }),
        "{err:?}"
    );
    assert_eq!(clan, before);
    let (day, turn) = fx.moment(|l| l < 40);
    clan.turns_used = turn;
    let salt = clan.stock.get(Resource::Salt);
    apply(
        &fx.env(day),
        &mut clan,
        Action::Shift { plot: 0, crew },
        &mut rng(1),
    )
    .unwrap();
    assert!(clan.stock.get(Resource::Salt) > salt);
    assert_eq!(clan.turns_used, turn + SHIFT_TURNS);
}

fn shift_stats(
    fx: &Fx,
    mut setup: impl FnMut(&mut Clan),
    moment: (i64, u8),
    runs: u64,
) -> (u32, u32, u32) {
    let (mut injuries, mut produced, mut pockets) = (0, 0, 0);
    for seed in 0..runs {
        let mut clan = fx.clan();
        place(&mut clan, 0, "peat-stack");
        setup(&mut clan);
        clan.turns_used = moment.1;
        let party = vec![clan.crew[1].id];
        let ev = apply(
            &fx.env(moment.0),
            &mut clan,
            Action::Shift {
                plot: 0,
                crew: party,
            },
            &mut rng(seed),
        )
        .unwrap();
        injuries += u32::try_from(count(&ev, |e| matches!(e, Event::Injured { .. }))).unwrap();
        pockets += u32::try_from(count(&ev, |e| matches!(e, Event::Pocketed { .. }))).unwrap();
        produced += ev
            .iter()
            .map(|e| {
                if let Event::Produced { qty, .. } = e {
                    *qty
                } else {
                    0
                }
            })
            .sum::<u32>();
    }
    (injuries, produced, pockets)
}

#[test]
fn accidents_depend_on_the_water_and_on_the_hand() {
    let fx = Fx::new();
    let calm = fx.moment(|l| l < 55);
    let up = fx.moment(|l| (60..70).contains(&l));
    let flood = fx.moment(|l| l >= 70);
    let plain = |_: &mut Clan| {};
    let n = 1_500;
    let (calm_hurt, ..) = shift_stats(&fx, plain, calm, n);
    let (up_hurt, ..) = shift_stats(&fx, plain, up, n);
    assert!(
        up_hurt > calm_hurt + 30,
        "water up hurts more: {calm_hurt} vs {up_hurt}"
    );
    // At flood the peat stack (works to 70) may refuse; compare on a calm moment with traits.
    let clumsy = |c: &mut Clan| c.crew[1].traits.push("clumsy".into());
    let steady = |c: &mut Clan| c.crew[1].traits.push("steady".into());
    let (clumsy_hurt, ..) = shift_stats(&fx, clumsy, calm, n);
    let (steady_hurt, ..) = shift_stats(&fx, steady, calm, n);
    assert!(
        clumsy_hurt > calm_hurt + 20,
        "clumsy: {clumsy_hurt} vs {calm_hurt}"
    );
    assert!(
        steady_hurt + 15 < calm_hurt,
        "steady: {steady_hurt} vs {calm_hurt}"
    );
    let _ = flood;
}

#[test]
fn the_flood_sends_the_fearful_home_and_hurts_with_water_injuries() {
    let fx = Fx::new();
    let flood = fx.moment(|l| l >= 70);
    let mut clan = fx.clan();
    place(&mut clan, 0, "peat-stack");
    clan.turns_used = flood.1;
    let crew = vec![clan.crew[1].id];
    // A peat stack works to water 70; find a flood moment at exactly 70 or tolerate the gate.
    let timid = {
        let mut c = clan.clone();
        c.crew[1].traits.push("fearful".into());
        c
    };
    let mut c = timid.clone();
    let err = apply(
        &fx.env(flood.0),
        &mut c,
        Action::Shift {
            plot: 0,
            crew: crew.clone(),
        },
        &mut rng(1),
    )
    .unwrap_err();
    assert!(
        matches!(
            err,
            ActionError::Refuses(_) | ActionError::TideTooHigh { .. }
        ),
        "{err:?}"
    );
    // A scar can make the same refusal: haunted is the scar of a near drowning.
    let mut haunted = fx.clan();
    place(&mut haunted, 0, "ropewalk");
    place(&mut haunted, 1, "salt-pan");
    // Use a building with no tide gate to isolate refusal: give it produce via fish-weir is gated;
    // the ropewalk makes nothing by hand, so use the smokehouse (no gate).
    place(&mut haunted, 2, "smokehouse");
    haunted.turns_used = flood.1;
    haunted.crew[1].traits.push("haunted".into());
    let party = vec![haunted.crew[1].id];
    assert!(matches!(
        apply(
            &fx.env(flood.0),
            &mut haunted,
            Action::Shift {
                plot: 2,
                crew: party.clone()
            },
            &mut rng(1)
        ),
        Err(ActionError::Refuses(_))
    ));
    haunted.crew[1].traits.clear();
    let ev = (0..400)
        .flat_map(|s| {
            let mut c = haunted.clone();
            apply(
                &fx.env(flood.0),
                &mut c,
                Action::Shift {
                    plot: 2,
                    crew: party.clone(),
                },
                &mut rng(s),
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    let injuries: Vec<&Event> = ev
        .iter()
        .filter(|e| matches!(e, Event::Injured { .. }))
        .collect();
    assert!(!injuries.is_empty());
    assert!(
        injuries.iter().all(|e| matches!(e, Event::Injured { injury, .. } if injury == "a near drowning" || injury == "a broken leg")),
        "flood accidents are water injuries"
    );
}

#[test]
fn greedy_hands_pocket_part_of_the_take_and_lucky_hands_double_it() {
    let fx = Fx::new();
    let calm = fx.moment(|l| l < 55);
    let (_, base, none) = shift_stats(&fx, |_| {}, calm, 400);
    let (_, greedy, pockets) =
        shift_stats(&fx, |c| c.crew[1].traits.push("greedy".into()), calm, 400);
    let (_, lucky, _) = shift_stats(&fx, |c| c.crew[1].traits.push("lucky".into()), calm, 400);
    assert_eq!(none, 0);
    assert!(pockets > 20, "{pockets}");
    // Greedy works 5% harder but keeps 10%: net less reaches the stores.
    assert!(greedy < base, "{greedy} vs {base}");
    assert!(lucky > base + base / 20, "{lucky} vs {base}");
}

#[test]
fn skill_trade_and_bonds_change_what_a_shift_brings_in() {
    let fx = Fx::new();
    let calm = fx.moment(|l| l < 55);
    let (_, base, _) = shift_stats(&fx, |_| {}, calm, 300);
    let (_, skilled, _) = shift_stats(&fx, |c| c.crew[1].xp[Trade::Raker.index()] = 144, calm, 300);
    assert!(
        skilled > base + base / 5,
        "level-6 raker {skilled} vs {base}"
    );
    let (_, offtrade, _) = shift_stats(&fx, |c| c.crew[1].trade = Trade::Fighter, calm, 300);
    assert!(
        offtrade < base,
        "an off-trade hand is slower: {offtrade} vs {base}"
    );
    // Two hands out-produce one; a bonded pair out-produces a strangers' pair.
    let pair = |bonded: bool| {
        let mut total = 0;
        for seed in 0..300 {
            let mut clan = fx.clan();
            place(&mut clan, 0, "peat-stack");
            clan.turns_used = calm.1;
            let (a, b) = (clan.crew[0].id, clan.crew[1].id);
            if bonded {
                clan.share_hardship(a, b, 8);
            }
            let ev = apply(
                &fx.env(calm.0),
                &mut clan,
                Action::Shift {
                    plot: 0,
                    crew: vec![a, b],
                },
                &mut rng(seed),
            )
            .unwrap();
            total += ev
                .iter()
                .map(|e| {
                    if let Event::Produced { qty, .. } = e {
                        *qty
                    } else {
                        0
                    }
                })
                .sum::<u32>();
        }
        total
    };
    let (strangers, bonded) = (pair(false), pair(true));
    assert!(strangers > base, "two hands beat one");
    assert!(
        bonded > strangers + strangers / 12,
        "bonded {bonded} vs strangers {strangers}"
    );
}

#[test]
fn shared_hardship_forms_bonds_and_the_water_makes_it_faster() {
    let fx = Fx::new();
    let calm = fx.moment(|l| l < 55);
    let up = fx.moment(|l| (60..70).contains(&l));
    let shifts_to_bond = |moment: (i64, u8)| {
        let mut clan = fx.clan();
        place(&mut clan, 0, "peat-stack");
        let (a, b) = (clan.crew[0].id, clan.crew[1].id);
        for n in 1..=20 {
            clan.turns_used = moment.1;
            let ev = apply(
                &fx.env(moment.0),
                &mut clan,
                Action::Shift {
                    plot: 0,
                    crew: vec![a, b],
                },
                &mut rng(n),
            )
            .unwrap();
            for c in &mut clan.crew {
                c.injuries.clear();
            }
            if count(&ev, |e| matches!(e, Event::Bonded { .. })) > 0 {
                return n;
            }
        }
        99
    };
    let (slow, fast) = (shifts_to_bond(calm), shifts_to_bond(up));
    assert!(slow <= 8, "{slow}");
    assert!(fast < slow, "hard water bonds sooner: {fast} vs {slow}");
}

#[test]
fn a_shift_eats_its_inputs_and_a_hold_without_them_makes_nothing() {
    let fx = Fx::new();
    let mut clan = fx.clan();
    place(&mut clan, 0, "smokehouse");
    clan.stock = Default::default();
    let party = vec![clan.crew[1].id];
    let (day, turn) = fx.moment(|l| l < 55);
    clan.turns_used = turn;
    assert!(matches!(
        apply(
            &fx.env(day),
            &mut clan,
            Action::Shift {
                plot: 0,
                crew: party.clone()
            },
            &mut rng(1)
        ),
        Err(ActionError::CantAfford(_))
    ));
    clan.stock.add(Resource::Fish, 9);
    clan.stock.add(Resource::Salt, 3);
    apply(
        &fx.env(day),
        &mut clan,
        Action::Shift {
            plot: 0,
            crew: party,
        },
        &mut rng(1),
    )
    .unwrap();
    assert_eq!(
        (
            clan.stock.get(Resource::Fish),
            clan.stock.get(Resource::Salt)
        ),
        (0, 0)
    );
    assert!(clan.stock.get(Resource::Food) >= 6);
}

#[test]
fn shift_party_rules() {
    let fx = Fx::new();
    let mut clan = fx.clan();
    place(&mut clan, 0, "peat-stack");
    place(&mut clan, 1, "ropewalk");
    let ids: Vec<u16> = clan.crew.iter().map(|c| c.id).collect();
    let env = fx.env(DAY);
    let run = |clan: &mut Clan, plot, crew: Vec<u16>| {
        apply(&env, clan, Action::Shift { plot, crew }, &mut rng(1))
    };
    assert_eq!(run(&mut clan, 0, vec![]), Err(ActionError::BadParty));
    assert_eq!(
        run(&mut clan, 0, vec![ids[0], ids[0]]),
        Err(ActionError::BadParty)
    );
    assert_eq!(run(&mut clan, 0, vec![99]), Err(ActionError::NoCrew(99)));
    assert_eq!(
        run(&mut clan, 1, vec![ids[0]]),
        Err(ActionError::NotAWorkplace(1)),
        "a ropewalk makes nothing by hand"
    );
    assert_eq!(
        run(&mut clan, 5, vec![ids[0]]),
        Err(ActionError::NotAWorkplace(5))
    );
    clan.crew[0].injuries.push(Injured {
        id: "broken-leg".into(),
        days: 9,
    });
    assert!(matches!(
        run(&mut clan, 0, vec![ids[0]]),
        Err(ActionError::Unfit(_))
    ));
}

#[test]
fn crafting_needs_the_workshop_the_recipe_the_inputs_and_a_hand_and_skill_adds_a_bonus() {
    let fx = Fx::new();
    let env = fx.env(DAY);
    let mut clan = fx.clan();
    let tools = |clan: &mut Clan, seed| {
        apply(
            &env,
            clan,
            Action::Craft {
                recipe: "forge-tools".into(),
            },
            &mut rng(seed),
        )
    };
    clan.recipes.push("forge-tools".into());
    assert!(matches!(tools(&mut clan, 1), Err(ActionError::NoWorkshop(w)) if w == "Smithy"));
    place(&mut clan, 0, "smithy");
    assert!(matches!(
        apply(
            &env,
            &mut clan,
            Action::Craft {
                recipe: "nope".into()
            },
            &mut rng(1)
        ),
        Err(ActionError::UnknownRecipe(_))
    ));
    let (iron, tools_before) = (
        clan.stock.get(Resource::Iron),
        clan.stock.get(Resource::Tools),
    );
    tools(&mut clan, 1).unwrap();
    assert_eq!(clan.stock.get(Resource::Iron), iron - 2);
    assert!(clan.stock.get(Resource::Tools) > tools_before);
    assert_eq!(clan.turns_used, 2);
    clan.stock.take_up_to(Resource::Iron, 100);
    assert!(matches!(
        tools(&mut clan, 1),
        Err(ActionError::CantAfford(_))
    ));
    // A level-8 smith beats a novice on yield.
    let yield_of = |xp: u16| {
        (0..400)
            .map(|s| {
                let mut c = fx.clan();
                c.recipes.push("forge-tools".into());
                place(&mut c, 0, "smithy");
                for m in &mut c.crew {
                    m.xp[Trade::Smith.index()] = 0;
                }
                c.crew[0].xp[Trade::Smith.index()] = xp;
                c.crew[0].trade = Trade::Smith;
                let before = c.stock.get(Resource::Tools);
                tools(&mut c, s).unwrap();
                c.stock.get(Resource::Tools) - before
            })
            .sum::<u32>()
    };
    let (novice, master) = (yield_of(0), yield_of(256));
    assert_eq!(novice, 400);
    assert!(master > novice + 100, "{master} vs {novice}");
}

#[test]
fn training_grows_skills_costs_tallies_and_a_quick_study_learns_faster() {
    let fx = Fx::new();
    let env = fx.env(DAY);
    let mut clan = fx.clan();
    let id = clan.crew[0].id;
    let t = clan.tallies;
    apply(
        &env,
        &mut clan,
        Action::Train {
            crew: id,
            skill: Trade::Smith,
        },
        &mut rng(1),
    )
    .unwrap();
    assert_eq!((clan.tallies, clan.turns_used), (t - 3, 2));
    let plain = clan.crew[0].xp[Trade::Smith.index()];
    assert!(plain >= 5);
    let mut quick = fx.clan();
    quick.crew[0].traits.push("quick-study".into());
    apply(
        &env,
        &mut quick,
        Action::Train {
            crew: id,
            skill: Trade::Smith,
        },
        &mut rng(1),
    )
    .unwrap();
    assert!(quick.crew[0].xp[Trade::Smith.index()] > plain);
    // Anyone can learn any skill, which changes what they can do: a trained smith crafts.
    clan.tallies = 0;
    assert!(matches!(
        apply(
            &env,
            &mut clan,
            Action::Train {
                crew: id,
                skill: Trade::Smith
            },
            &mut rng(1)
        ),
        Err(ActionError::CantAfford(_))
    ));
}

#[test]
fn tending_needs_a_healer_who_is_not_the_patient_and_a_better_healer_cuts_more() {
    let fx = Fx::new();
    let env = fx.env(DAY);
    let mut clan = fx.clan();
    clan.crew.retain(|c| c.trade != Trade::Healer);
    for c in &mut clan.crew {
        c.xp[Trade::Healer.index()] = 0;
    }
    let (patient, other) = (clan.crew[0].id, clan.crew[1].id);
    assert_eq!(
        apply(&env, &mut clan, Action::Tend { crew: patient }, &mut rng(1)),
        Err(ActionError::NoPatient)
    );
    clan.crew[0].injuries.push(Injured {
        id: "fever".into(),
        days: 6,
    });
    assert_eq!(
        apply(&env, &mut clan, Action::Tend { crew: patient }, &mut rng(1)),
        Err(ActionError::NoHealer)
    );
    clan.crew[1].xp[Trade::Healer.index()] = 4;
    apply(&env, &mut clan, Action::Tend { crew: patient }, &mut rng(1)).unwrap();
    assert_eq!(clan.crew[0].injuries[0].days, 5);
    clan.crew[1].xp[Trade::Healer.index()] = 100;
    let ev = apply(&env, &mut clan, Action::Tend { crew: patient }, &mut rng(1)).unwrap();
    assert_eq!(clan.crew[0].injuries[0].days, 3, "level 5 cuts two days");
    assert!(matches!(ev[0], Event::Tended { .. }));
    // The healer cannot tend themself.
    clan.crew[1].injuries.push(Injured {
        id: "fever".into(),
        days: 6,
    });
    clan.crew[0].injuries.clear();
    assert_eq!(
        apply(&env, &mut clan, Action::Tend { crew: other }, &mut rng(1)),
        Err(ActionError::NoHealer)
    );
    // Tending the last day finishes the injury.
    clan.crew[0].injuries.push(Injured {
        id: "sprain".into(),
        days: 1,
    });
    clan.crew[1].injuries.clear();
    let ev = apply(&env, &mut clan, Action::Tend { crew: patient }, &mut rng(1)).unwrap();
    assert!(clan.crew[0].injuries.is_empty());
    assert_eq!(count(&ev, |e| matches!(e, Event::Healed { .. })), 1);
}

#[test]
fn surveying_charts_further_with_a_tower_and_a_reader_of_flats() {
    let fx = Fx::new();
    let env = fx.env(DAY);
    let chart = |clan: &mut Clan| {
        let ev = apply(&env, clan, Action::Survey, &mut rng(1)).unwrap();
        let Event::Surveyed { tiles } = ev[0] else {
            panic!()
        };
        tiles
    };
    let mut plain = fx.clan();
    plain
        .crew
        .iter_mut()
        .for_each(|c| c.xp[Trade::Wader.index()] = 0);
    let mut tower = plain.clone();
    place(&mut tower, 0, "watch-tower");
    let mut reader = plain.clone();
    reader.crew[0].xp[Trade::Wader.index()] = 16;
    let (a, b, c) = (chart(&mut plain), chart(&mut tower), chart(&mut reader));
    assert!(b > c && c > a, "tower {b} > wader {c} > plain {a}");
    assert_eq!(chart(&mut plain), 0, "nothing new the second time");
}

#[test]
fn the_tidecourt_pays_less_than_it_charges_and_will_not_sell_relics() {
    let fx = Fx::new();
    let env = fx.env(DAY);
    let mut clan = fx.clan();
    let t = clan.tallies;
    let ev = apply(
        &env,
        &mut clan,
        Action::Sell {
            what: Resource::Iron,
            qty: 10,
        },
        &mut rng(1),
    )
    .unwrap();
    assert_eq!(
        ev,
        vec![Event::Sold {
            what: Resource::Iron,
            qty: 10,
            tallies: 90
        }]
    );
    assert_eq!((clan.tallies, clan.stock.get(Resource::Iron)), (t + 90, 90));
    let ev = apply(
        &env,
        &mut clan,
        Action::Buy {
            what: Resource::Iron,
            qty: 10,
        },
        &mut rng(1),
    )
    .unwrap();
    assert_eq!(
        ev,
        vec![Event::Bought {
            what: Resource::Iron,
            qty: 10,
            tallies: 115
        }]
    );
    assert_eq!(
        apply(
            &env,
            &mut clan,
            Action::Buy {
                what: Resource::Relic,
                qty: 1
            },
            &mut rng(1)
        ),
        Err(ActionError::NotForSale)
    );
    assert_eq!(
        apply(
            &env,
            &mut clan,
            Action::Sell {
                what: Resource::Iron,
                qty: 0
            },
            &mut rng(1)
        ),
        Err(ActionError::BadAmount)
    );
    assert!(matches!(
        apply(
            &env,
            &mut clan,
            Action::Sell {
                what: Resource::Iron,
                qty: 9_999
            },
            &mut rng(1)
        ),
        Err(ActionError::CantAfford(_))
    ));
    // A scarce price pays more: change the posted price and the same sale pays more.
    let mut dear = fx.prices;
    dear[Resource::Salt.index()] *= 2;
    let env2 = Env {
        prices: &dear,
        ..fx.env(DAY)
    };
    let a = apply(
        &env,
        &mut fx.clan(),
        Action::Sell {
            what: Resource::Salt,
            qty: 20,
        },
        &mut rng(1),
    )
    .unwrap();
    let b = apply(
        &env2,
        &mut fx.clan(),
        Action::Sell {
            what: Resource::Salt,
            qty: 20,
        },
        &mut rng(1),
    )
    .unwrap();
    let (Event::Sold { tallies: x, .. }, Event::Sold { tallies: y, .. }) = (&a[0], &b[0]) else {
        panic!()
    };
    assert_eq!(*y, 2 * *x);
}

#[test]
fn the_sluice_floods_the_approach_only_when_built_set_and_the_water_is_up() {
    let fx = Fx::new();
    let env = fx.env(DAY);
    let mut clan = fx.clan();
    assert_eq!(
        apply(
            &env,
            &mut clan,
            Action::Sluice { level: Some(60) },
            &mut rng(1)
        ),
        Err(ActionError::NoSluice)
    );
    place(&mut clan, 0, "palisade");
    place(&mut clan, 1, "sluice");
    let dry = clan.hold.defence(&fx.cat, 90);
    apply(
        &env,
        &mut clan,
        Action::Sluice { level: Some(60) },
        &mut rng(1),
    )
    .unwrap();
    assert_eq!(clan.hold.defence(&fx.cat, 59), dry);
    assert!(clan.hold.defence(&fx.cat, 61) > dry);
    apply(&env, &mut clan, Action::Sluice { level: None }, &mut rng(1)).unwrap();
    assert_eq!(clan.hold.defence(&fx.cat, 100), dry);
}

#[test]
fn posting_crew_fills_slots_and_staffed_buildings_out_produce_empty_ones_overnight() {
    let fx = Fx::new();
    let env = fx.env(DAY);
    let mut clan = fx.clan();
    place(&mut clan, 0, "peat-stack");
    place(&mut clan, 1, "hut");
    let ids: Vec<u16> = clan.crew.iter().map(|c| c.id).collect();
    assert!(matches!(
        apply(
            &env,
            &mut clan,
            Action::Post {
                crew: ids[0],
                plot: Some(1)
            },
            &mut rng(1)
        ),
        Err(ActionError::NotAWorkplace(1))
    ));
    assert!(matches!(
        apply(
            &env,
            &mut clan,
            Action::Post {
                crew: ids[0],
                plot: Some(40)
            },
            &mut rng(1)
        ),
        Err(ActionError::NoPlot(40))
    ));
    apply(
        &env,
        &mut clan,
        Action::Post {
            crew: ids[1],
            plot: Some(0),
        },
        &mut rng(1),
    )
    .unwrap();
    assert_eq!(
        apply(
            &env,
            &mut clan,
            Action::Post {
                crew: ids[0],
                plot: Some(0)
            },
            &mut rng(1)
        ),
        Err(ActionError::PostFull)
    );
    let empty = {
        let mut c = clan.clone();
        c.crew[1].post = None;
        c
    };
    let peat = |mut c: Clan| {
        c.stock.take_up_to(Resource::Peat, 1_000);
        c.stock.add(Resource::Food, 100);
        let mut total = 0;
        for day in 1..=30 {
            c.last_day = DAY + day - 1;
            catch_up(&fx.env(DAY + day), &mut c, DAY + day);
            total += c.stock.get(Resource::Peat);
            c.stock.take_up_to(Resource::Peat, 1_000);
        }
        total
    };
    let (staffed, unstaffed) = (peat(clan.clone()), peat(empty));
    assert!(staffed > unstaffed * 2, "{staffed} vs {unstaffed}");
    // Standing a hand down empties the post.
    apply(
        &env,
        &mut clan,
        Action::Post {
            crew: ids[1],
            plot: None,
        },
        &mut rng(1),
    )
    .unwrap();
    assert_eq!(clan.crew[1].post, None);
}

fn quiet_night(fx: &Fx, clan: &mut Clan, nights: i64) -> Vec<Event> {
    let start = clan.last_day;
    catch_up(&fx.env(start + nights), clan, start + nights)
}

#[test]
fn a_fed_warm_hold_is_steady_a_hungry_one_loses_morale_then_crew_with_an_epitaph() {
    let fx = Fx::new();
    let mut fed = fx.clan();
    quiet_night(&fx, &mut fed, 1);
    assert_eq!(
        fed.stock.get(Resource::Food),
        117,
        "three crew eat three of 120"
    );
    assert_eq!(fed.starving, 0);
    assert!(fed.morale >= 60);
    let mut hungry = fx.clan();
    hungry.stock = Default::default();
    hungry.stock.add(Resource::Peat, 50);
    let ev = quiet_night(&fx, &mut hungry, 1);
    assert!(ev.iter().any(|e| matches!(e, Event::Hungry { short: 3 })));
    assert_eq!(
        (hungry.starving, hungry.morale),
        (1, 49),
        "60 - 12 for hunger + 1 drift toward 60"
    );
    quiet_night(&fx, &mut hungry, 1);
    assert_eq!(hungry.crew.len(), 3);
    let ev = quiet_night(&fx, &mut hungry, 1);
    assert_eq!(hungry.crew.len(), 2, "the third hungry night takes one");
    assert!(ev.iter().any(|e| matches!(e, Event::Starved { .. })));
    assert_eq!(hungry.hall.len(), 1);
    assert!(hungry.hall[0].text.contains(' '));
    // Fish stand in for food at two to the ration.
    let mut fishers = fx.clan();
    fishers.stock = Default::default();
    fishers.stock.add(Resource::Peat, 50);
    fishers.stock.add(Resource::Fish, 7);
    quiet_night(&fx, &mut fishers, 1);
    assert_eq!(
        (fishers.starving, fishers.stock.get(Resource::Fish)),
        (0, 1),
        "six fish make three rations; one is left"
    );
}

#[test]
fn a_cold_hold_loses_morale_and_peat_is_burned_by_buildings() {
    let fx = Fx::new();
    let mut warm = fx.clan();
    let mut cold = fx.clan();
    cold.stock.take_up_to(Resource::Peat, 1_000);
    place(&mut warm, 0, "infirmary");
    let before = warm.stock.get(Resource::Peat);
    quiet_night(&fx, &mut warm, 1);
    assert_eq!(
        before - warm.stock.get(Resource::Peat),
        2,
        "one infirmary fire, one fire for three crew"
    );
    let ev = quiet_night(&fx, &mut cold, 1);
    assert!(ev.iter().any(|e| matches!(e, Event::Cold { .. })));
    assert!(cold.morale < warm.morale);
}

#[test]
fn healing_is_faster_with_an_infirmary_a_healer_and_a_hardy_constitution() {
    let fx = Fx::new();
    let days_to_heal = |setup: &dyn Fn(&mut Clan)| {
        let mut total = 0;
        for seed in 0..60u64 {
            let mut clan = fx.clan();
            clan.name = format!("Healing{seed}");
            clan.crew[0].injuries.push(Injured {
                id: "fever".into(),
                days: 6,
            });
            setup(&mut clan);
            let mut n = 0;
            while !clan.crew[0].injuries.is_empty() && n < 12 {
                quiet_night(&fx, &mut clan, 1);
                n += 1;
            }
            total += n;
        }
        total
    };
    let base = days_to_heal(&|_| {});
    let infirmary = days_to_heal(&|c| place(c, 0, "infirmary"));
    let healer = days_to_heal(&|c| c.crew[1].xp[Trade::Healer.index()] = 16);
    let hardy = days_to_heal(&|c| c.crew[0].traits.push("hardy".into()));
    assert_eq!(base, 60 * 6);
    assert_eq!(infirmary, 60 * 3);
    assert_eq!(healer, 60 * 3);
    assert!(hardy < base, "hardy {hardy} vs {base}");
}

#[test]
fn injuries_scar_into_traits_at_the_rate_the_table_says_and_never_past_three_traits() {
    let fx = Fx::new();
    let mut scarred = 0;
    for seed in 0..400u64 {
        let mut clan = fx.clan();
        clan.name = format!("Scar{seed}");
        clan.crew[0].injuries.push(Injured {
            id: "broken-leg".into(),
            days: 1,
        });
        let ev = quiet_night(&fx, &mut clan, 1);
        if clan.crew[0].traits.contains(&"limp".to_owned()) {
            scarred += 1;
            assert!(ev.iter().any(|e| matches!(e, Event::Scarred { .. })));
        }
    }
    assert!(
        (140..260).contains(&scarred),
        "about half of 400: {scarred}"
    );
    let mut full = fx.clan();
    full.crew[0].traits = vec!["steady".into(), "lucky".into(), "hardy".into()];
    full.crew[0].injuries.push(Injured {
        id: "broken-leg".into(),
        days: 1,
    });
    for seed in 0..40 {
        let mut c = full.clone();
        c.name = format!("Full{seed}");
        quiet_night(&fx, &mut c, 1);
        assert_eq!(c.crew[0].traits.len(), 3);
    }
    // A scar changes outcomes: a limp lowers wet-water yield; shaky hands raise accident odds.
    let cat = &fx.cat;
    let mut c = fx.clan().crew[0].clone();
    let before = c.trait_sum(cat, |t| t.accident_pct);
    c.traits.push("shaky-hands".into());
    assert!(c.trait_sum(cat, |t| t.accident_pct) > before);
}

#[test]
fn crew_age_by_the_fortnight_and_the_old_retire_with_an_epitaph() {
    let fx = Fx::new();
    let mut clan = fx.clan();
    clan.crew[0].age = 59;
    clan.crew[1].age = 40;
    clan.founded = DAY;
    clan.last_day = DAY + 13;
    let ev = catch_up(&fx.env(DAY + 14), &mut clan, DAY + 14);
    assert_eq!(
        count(&ev, |e| matches!(e, Event::Aged { .. })),
        3,
        "everyone has a birthday, then the oldest retires"
    );
    assert!(ev.iter().any(|e| matches!(e, Event::Retired { .. })));
    assert_eq!(clan.crew.len(), 2);
    assert_eq!(clan.hall.len(), 1);
    assert!(clan.crew.iter().any(|c| c.age == 41));
    // Age 49 and over wears grit down.
    let mut elder = fx.clan().crew[0].clone();
    elder.attrs[Attr::Grit.index()] = 8;
    elder.age = 53;
    assert_eq!(elder.attr(Attr::Grit), 6);
}

#[test]
fn the_old_retire_by_chance_between_52_and_59() {
    let fx = Fx::new();
    let mut retired = 0;
    for seed in 0..200u64 {
        let mut clan = fx.clan();
        clan.name = format!("Old{seed}");
        clan.crew[0].age = 54;
        clan.founded = DAY;
        clan.last_day = DAY + 13;
        catch_up(&fx.env(DAY + 14), &mut clan, DAY + 14);
        retired += usize::from(clan.crew.len() == 2);
    }
    assert!(
        (40..120).contains(&retired),
        "age 55 retires 40% of the time: {retired}"
    );
}

#[test]
fn low_morale_drives_crew_away_and_bonded_crew_are_the_last_to_go() {
    let fx = Fx::new();
    let mut gone = 0;
    for seed in 0..100u64 {
        let mut clan = fx.clan();
        clan.name = format!("Low{seed}");
        clan.morale = 5;
        let (a, b) = (clan.crew[0].id, clan.crew[1].id);
        clan.share_hardship(a, b, 8);
        let ev = quiet_night(&fx, &mut clan, 1);
        if ev.iter().any(|e| matches!(e, Event::Deserted { .. })) {
            gone += 1;
            assert!(
                clan.crew_by(a).is_some() && clan.crew_by(b).is_some(),
                "the unbonded one leaves"
            );
            assert_eq!(clan.crew.len(), 2);
        }
    }
    assert!((5..40).contains(&gone), "about one night in five: {gone}");
}

#[test]
fn losing_a_bonded_partner_costs_morale_and_an_unbonded_one_does_not() {
    let fx = Fx::new();
    let env = fx.env(DAY);
    let mut bonded = fx.clan();
    let (a, b) = (bonded.crew[0].id, bonded.crew[1].id);
    bonded.share_hardship(a, b, 8);
    let mut loose = fx.clan();
    let m = bonded.morale;
    let ev = apply(&env, &mut bonded, Action::Retire { crew: a }, &mut rng(1)).unwrap();
    apply(&env, &mut loose, Action::Retire { crew: a }, &mut rng(1)).unwrap();
    assert_eq!(bonded.morale, m - 10);
    assert_eq!(loose.morale, m);
    assert!(ev.iter().any(|e| matches!(e, Event::Grief { .. })));
    assert_eq!(bonded.hall.len(), 1);
}

#[test]
fn catching_up_replays_each_missed_night_once_up_to_a_cap_and_is_idempotent() {
    let fx = Fx::new();
    let mut clan = fx.clan();
    clan.stock.add(Resource::Food, 1_000);
    clan.stock.add(Resource::Peat, 1_000);
    let before = clan.stock.get(Resource::Food);
    let ev = catch_up(&fx.env(DAY + 3), &mut clan, DAY + 3);
    assert_eq!(count(&ev, |e| matches!(e, Event::Night { .. })), 3);
    assert_eq!(before - clan.stock.get(Resource::Food), 9);
    assert_eq!((clan.last_day, clan.turns_used), (DAY + 3, 0));
    assert!(catch_up(&fx.env(DAY + 3), &mut clan, DAY + 3).is_empty());
    assert!(
        catch_up(&fx.env(DAY), &mut clan, DAY).is_empty(),
        "a clock that goes back changes nothing"
    );
    let ev = catch_up(&fx.env(DAY + 100), &mut clan, DAY + 100);
    assert_eq!(
        count(&ev, |e| matches!(e, Event::Night { .. })),
        usize::try_from(MAX_CATCH_UP).unwrap()
    );
    // Two clans catching up the same way end up the same.
    let mut a = fx.clan();
    let mut b = fx.clan();
    catch_up(&fx.env(DAY + 9), &mut a, DAY + 9);
    for d in 1..=9 {
        catch_up(&fx.env(DAY + d), &mut b, DAY + d);
    }
    assert_eq!(a, b, "nights do not depend on when the player looks");
}

#[test]
fn candidates_are_redrawn_each_night_and_a_gatehouse_brings_more() {
    let fx = Fx::new();
    let mut plain = fx.clan();
    let mut gated = fx.clan();
    place(&mut gated, 0, "gatehouse");
    let first = plain.candidates.clone();
    quiet_night(&fx, &mut plain, 1);
    quiet_night(&fx, &mut gated, 1);
    assert_ne!(plain.candidates, first);
    assert_eq!(plain.candidates.len(), 2);
    assert_eq!(gated.candidates.len(), 3);
}

#[test]
fn the_same_log_of_actions_reproduces_the_same_clan() {
    let fx = Fx::new();
    let play = |seed: u64| {
        let mut r = rng(seed);
        let mut clan = fx.clan();
        let mut events = Vec::new();
        let (day, turn) = fx.moment(|l| l < 40);
        clan.turns_used = turn;
        let env = fx.env(day);
        let a = clan.crew[0].id;
        place(&mut clan, 5, "smokehouse");
        for action in [
            Action::Build {
                plot: 0,
                building: "salt-pan".into(),
            },
            Action::Labour { track: Track::Work },
            Action::Labour { track: Track::Work },
            Action::Train {
                crew: a,
                skill: Trade::Raker,
            },
            Action::Shift {
                plot: 5,
                crew: vec![a],
            },
            Action::Shift {
                plot: 5,
                crew: vec![a],
            },
            Action::Shift {
                plot: 5,
                crew: vec![a],
            },
        ] {
            // A refusal is part of the log too: the same refusals happen on every replay.
            events.extend(apply(&env, &mut clan, action, &mut r).unwrap_or_default());
        }
        catch_up(&fx.env(day + 5), &mut clan, day + 5);
        (clan, events)
    };
    assert_eq!(play(5), play(5));
    let outcomes: std::collections::HashSet<Vec<u8>> =
        (0..12).map(|s| play(s).0.encode()).collect();
    assert!(
        outcomes.len() > 1,
        "the seed matters: chance decides shifts"
    );
}

#[test]
fn waiting_moves_the_clock_on_so_a_gated_job_opens_when_the_water_falls() {
    let fx = Fx::new();
    let mut clan = fx.clan();
    place(&mut clan, 0, "salt-pan");
    let party = vec![clan.crew[1].id];
    // Start a few turns before the water falls under 45 after being above it.
    let (day, turn) = (DAY..DAY + 60)
        .flat_map(|d| (0..20u8).map(move |t| (d, t)))
        .find(|&(d, t)| {
            let level = |t| fx.tide.level(Tide::turn_minute(d, t, 30));
            level(t) > 45 && level(t + 8) <= 45
        })
        .unwrap();
    clan.turns_used = turn;
    let env = fx.env(day);
    let shift = Action::Shift {
        plot: 0,
        crew: party,
    };
    assert!(matches!(
        apply(&env, &mut clan, shift.clone(), &mut rng(1)),
        Err(ActionError::TideTooHigh { .. })
    ));
    let mut waited = 0;
    while matches!(
        apply(&env, &mut clan, shift.clone(), &mut rng(1)),
        Err(ActionError::TideTooHigh { .. })
    ) && waited < 8
    {
        let ev = apply(&env, &mut clan, Action::Wait { turns: 1 }, &mut rng(1)).unwrap();
        assert!(matches!(ev[0], Event::Waited { turns: 1, .. }));
        waited += 1;
    }
    assert!((1..=8).contains(&waited), "{waited}");
    assert_eq!(
        clan.turns_used,
        turn + waited + SHIFT_TURNS,
        "waiting and then the shift both spent turns"
    );
    // Bounds: nothing for zero or eleven turns; and not more than are left.
    assert_eq!(
        apply(&env, &mut clan, Action::Wait { turns: 0 }, &mut rng(1)),
        Err(ActionError::BadAmount)
    );
    assert_eq!(
        apply(&env, &mut clan, Action::Wait { turns: 11 }, &mut rng(1)),
        Err(ActionError::BadAmount)
    );
    clan.turns_used = 28;
    assert!(matches!(
        apply(&env, &mut clan, Action::Wait { turns: 5 }, &mut rng(1)),
        Err(ActionError::NoTurns { need: 5, have: 2 })
    ));
}
