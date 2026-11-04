//! Skirmish rules: the rows, the six actions, one special per trade, terrain and tide, flight,
//! quick-resolve, and invariants that must hold for any fight.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::too_many_lines,
    clippy::many_single_char_names
)]

use super::*;
use crate::crew::Injured;

fn cat() -> Catalog {
    Catalog::embedded().unwrap()
}

fn rng(seed: u64) -> DoorRng {
    DoorRng::from_seed(seed)
}

fn unit(side: Side, name: &str, row: u8, hp: i16, atk: i16, def: i16, spd: i16) -> Unit {
    let mut u = Unit::blank(side, name.to_owned());
    u.hp = hp;
    u.max_hp = hp;
    u.atk = atk;
    u.def = def;
    u.spd = spd;
    u.row = row;
    if side == Side::Party {
        u.crew = 1;
    }
    u
}

fn party(name: &str, trade: Trade, row: u8) -> Unit {
    let mut u = unit(Side::Party, name, row, 20, 4, 1, 5);
    u.trade = Some(trade);
    u
}

fn foe(name: &str, row: u8, hp: i16) -> Unit {
    unit(Side::Foe, name, row, hp, 3, 0, 1)
}

fn board(units: Vec<Unit>) -> Fight {
    let mut f = Fight {
        group: "test".into(),
        units,
        round: 1,
        order: Vec::new(),
        cursor: 0,
        wet: false,
        level: 40,
        limit: 0,
        flee_mod: 0,
    };
    f.compact();
    f
}

/// Make it unit `i`'s turn.
fn turn(f: &mut Fight, i: usize) {
    f.order = vec![u8::try_from(i).unwrap()];
    f.cursor = 0;
}

fn act(f: &mut Fight, i: usize, a: FightAction, seed: u64) -> Result<Vec<String>, String> {
    turn(f, i);
    let mut lines = Vec::new();
    // Stop the follow-on round so a test looks at one action.
    f.round = MAX_ROUNDS;
    f.act(a, &mut rng(seed), &mut lines).map(|_| lines)
}

fn at(i: usize) -> u8 {
    u8::try_from(i).unwrap()
}

#[test]
fn only_the_front_rank_strikes_hand_to_hand_the_back_rank_throws_and_arms_reach_a_row_deeper() {
    // Party: fighter (front), healer (back). Foes: a wight in front, a hound behind it.
    let mut f = board(vec![
        party("Fighter", Trade::Fighter, 0),
        party("Healer", Trade::Healer, 2),
        foe("Wight", 0, 30),
        foe("Hound", 1, 30),
    ]);
    let (fighter, healer, wight, hound) = (0, 1, 2, 3);
    assert!(f.in_reach(fighter) && !f.in_reach(healer));
    assert!(
        act(&mut f, healer, FightAction::Strike { target: at(wight) }, 1)
            .unwrap_err()
            .contains("front rank")
    );
    assert!(
        act(
            &mut f,
            fighter,
            FightAction::Strike { target: at(hound) },
            1
        )
        .unwrap_err()
        .contains("behind the front rank")
    );
    assert_eq!(f.units[wight].hp, 30, "refusals change nothing");
    // The back rank can throw at anyone, and the throw lands (or misses) on the hound.
    let mut hit = false;
    for seed in 0..30 {
        f.units[hound].hp = 30;
        let before = f.units[hound].hp;
        act(
            &mut f,
            healer,
            FightAction::Throw { target: at(hound) },
            seed,
        )
        .unwrap();
        hit |= f.units[hound].hp < before;
    }
    assert!(hit);
    // With arms the fighter reaches the hound behind the wight.
    f.units[fighter].armed = true;
    assert_eq!(f.melee_targets(fighter), vec![wight, hound]);
    f.units[hound].hp = 30;
    let mut struck = false;
    for seed in 0..30 {
        f.units[hound].hp = 30;
        act(
            &mut f,
            fighter,
            FightAction::Strike { target: at(hound) },
            seed,
        )
        .unwrap();
        struck |= f.units[hound].hp < 30;
    }
    assert!(struck);
}

#[test]
fn a_strike_is_the_attack_less_the_armour_and_bracing_helps() {
    let dmg = |def: i16, braced: bool, hexed: u8, wet: bool| -> f64 {
        let mut total = 0.0;
        let n: u32 = 400;
        for seed in 0..u64::from(n) {
            let mut f = board(vec![unit(Side::Party, "A", 0, 99, 6, 0, 5), {
                let mut u = foe("B", 0, 99);
                u.def = def;
                u.braced = braced;
                u.hexed = hexed;
                u
            }]);
            f.wet = wet;
            f.units[0].trade = Some(Trade::Fighter);
            act(&mut f, 0, FightAction::Strike { target: 1 }, seed).unwrap();
            total += f64::from(99 - f.units[1].hp);
        }
        total / f64::from(n)
    };
    let plain = dmg(0, false, 0, false);
    assert!(
        dmg(2, false, 3, false) > dmg(2, false, 0, false),
        "a hexed foe's armour is thinner"
    );
    assert!(
        (5.0..7.5).contains(&plain),
        "6 attack, 0 to 2 variance, 10% misses: {plain}"
    );
    assert!(dmg(2, false, 0, false) < plain - 1.4, "armour soaks");
    assert!(
        dmg(0, true, 0, false) < plain - 2.0,
        "a braced foe takes less"
    );
    assert!(
        dmg(0, false, 0, true) < plain,
        "knee-deep water spoils the swing"
    );
}

#[test]
fn shoving_swaps_ranks_and_the_water_takes_a_light_foe_from_the_back() {
    let mut moved = false;
    for seed in 0..40 {
        let mut f = board(vec![
            {
                let mut u = party("Fighter", Trade::Fighter, 0);
                u.mass = 10;
                u
            },
            {
                let mut u = foe("Front", 0, 30);
                u.mass = 1;
                u
            },
            foe("Second", 1, 30),
        ]);
        act(&mut f, 0, FightAction::Shove { target: 1 }, seed).unwrap();
        if f.units[1].row == 1 {
            assert_eq!(f.units[2].row, 0, "the one behind comes forward");
            moved = true;
            break;
        }
    }
    assert!(moved, "a heavy hand shoves a light foe");
    // In the water, a light land-bound foe with nothing behind it goes under; an eel does not.
    let drowns = |aquatic: bool, wet: bool| -> bool {
        (0..40).any(|seed| {
            let mut f = board(vec![
                {
                    let mut u = party("Fighter", Trade::Fighter, 0);
                    u.mass = 10;
                    u
                },
                {
                    let mut l = foe("Light", 0, 30);
                    l.mass = 2;
                    l.aquatic = aquatic;
                    l
                },
            ]);
            f.wet = wet;
            act(&mut f, 0, FightAction::Shove { target: 1 }, seed).unwrap();
            f.units[1].gone
        })
    };
    assert!(drowns(false, true), "the current takes a light foe");
    assert!(
        !drowns(false, false),
        "on dry ground there is nowhere to go"
    );
    assert!(!drowns(true, true), "an eel is at home in the water");
}

#[test]
fn every_trade_has_one_special_that_changes_the_fight_and_waits_to_be_used_again() {
    let base = || {
        vec![
            party("P", Trade::Fighter, 0),
            foe("Big", 0, 40),
            foe("Other", 1, 40),
        ]
    };
    let with = |trade: Trade| {
        let mut v = base();
        v[0].trade = Some(trade);
        v[0].armed = true;
        board(v)
    };
    // Wader: the mire holds a foe a round (two in the water).
    let mut f = with(Trade::Wader);
    act(&mut f, 0, FightAction::Special { target: Some(1) }, 1).unwrap();
    assert_eq!(f.units[1].netted, 1);
    let mut w = with(Trade::Wader);
    w.wet = true;
    act(&mut w, 0, FightAction::Special { target: Some(1) }, 1).unwrap();
    assert_eq!(w.units[1].netted, 2);
    // Salvager: the net holds two.
    let mut f = with(Trade::Salvager);
    act(&mut f, 0, FightAction::Special { target: Some(2) }, 1).unwrap();
    assert_eq!(f.units[2].netted, 2);
    // Reed-witch: the hex.
    let mut f = with(Trade::ReedWitch);
    act(&mut f, 0, FightAction::Special { target: Some(1) }, 1).unwrap();
    assert_eq!(f.units[1].hexed, 3);
    assert!(f.atk_of(&f.units[1]) < f.units[1].atk);
    // Smith: the shield-wall guards the whole party.
    let mut v = base();
    v.insert(1, party("Q", Trade::Healer, 2));
    v[0].trade = Some(Trade::Smith);
    let mut f = board(v);
    act(&mut f, 0, FightAction::Special { target: None }, 1).unwrap();
    assert!(f.units[0].guarded > 0 && f.units[1].guarded > 0);
    assert_eq!(f.def_of(&f.units[1]), f.units[1].def + 2);
    // Healer: the salve heals everyone a little.
    let mut v = base();
    v.insert(1, party("Q", Trade::Raker, 1));
    v[0].trade = Some(Trade::Healer);
    v[0].row = 2;
    v[0].hp = 10;
    v[1].hp = 10;
    let mut f = board(v);
    act(&mut f, 0, FightAction::Special { target: None }, 1).unwrap();
    assert!(f.units[0].hp > 10 && f.units[1].hp > 10);
    // Raker: the sweep hits the whole front rank.
    let mut v = base();
    v[2].row = 0;
    v[0].trade = Some(Trade::Raker);
    v[0].atk = 12;
    let mut f = board(v);
    act(&mut f, 0, FightAction::Special { target: None }, 1).unwrap();
    assert!(f.units[1].hp < 40 && f.units[2].hp < 40);
    // Fighter: a cleave that ignores armour and leaves the swordsman open.
    let mut v = base();
    v[1].def = 4;
    v[0].atk = 6;
    let mut f = board(v);
    let before = f.def_of(&f.units[0]);
    let mut hit = 0;
    for seed in 0..30 {
        f.units[1].hp = 40;
        f.units[0].cool = 0;
        act(&mut f, 0, FightAction::Special { target: Some(1) }, seed).unwrap();
        if f.units[1].hp < 40 {
            assert!(
                40 - f.units[1].hp >= 9,
                "6 x 1.5, no armour: {}",
                40 - f.units[1].hp
            );
            hit += 1;
        }
        assert!(f.units[0].exposed && f.def_of(&f.units[0]) < before);
    }
    assert!(hit > 20);
    // Boatwright: the hook pulls a foe from the back to the front and clips it.
    let mut f = with(Trade::Boatwright);
    f.units[1].row = 0;
    f.units[2].row = 1;
    act(&mut f, 0, FightAction::Special { target: Some(2) }, 1).unwrap();
    assert_eq!((f.units[2].row, f.units[1].row), (0, 1));
    assert!(f.units[2].hp < 40);
    // After use, the special rests for a few rounds, and a refusal says how many.
    let mut f = with(Trade::ReedWitch);
    act(&mut f, 0, FightAction::Special { target: Some(1) }, 1).unwrap();
    assert_eq!(f.units[0].cool, SPECIAL_COOL + 1);
    let err = act(&mut f, 0, FightAction::Special { target: Some(1) }, 1).unwrap_err();
    assert!(err.contains("Not ready for 4 more rounds"), "{err}");
    f.units[0].cool = 1;
    assert!(
        act(&mut f, 0, FightAction::Special { target: Some(1) }, 1)
            .unwrap_err()
            .contains("1 more round.")
    );
    // A special that needs a target refuses without one, or with a corpse.
    let mut f = with(Trade::Salvager);
    assert!(act(&mut f, 0, FightAction::Special { target: None }, 1).is_err());
    f.units[1].hp = 0;
    assert!(act(&mut f, 0, FightAction::Special { target: Some(1) }, 1).is_err());
}

#[test]
fn a_netted_unit_loses_its_turn_and_mending_lifts_the_fallen() {
    let mut v = vec![party("P", Trade::Fighter, 0), foe("F", 0, 30)];
    v[1].netted = 1;
    v[1].atk = 9;
    let mut f = board(v);
    f.round = 1;
    f.order = vec![1];
    f.cursor = 0;
    let mut lines = Vec::new();
    f.advance(&mut rng(1), &mut lines);
    assert!(lines.iter().any(|l| l.contains("tangled")), "{lines:?}");
    assert_eq!(
        f.units[0].hp, 20,
        "and the blow it would have struck is not struck"
    );
    assert_eq!(f.units[1].netted, 0);

    let mut v = vec![
        party("Healer", Trade::Healer, 2),
        party("Down", Trade::Raker, 1),
        foe("F", 0, 30),
    ];
    v[1].hp = 0;
    let mut f = board(v);
    assert!(f.units[1].downed());
    act(&mut f, 0, FightAction::Mend { ally: 1 }, 1).unwrap();
    assert!(f.units[1].alive(), "the fallen is up again");
    // An unhurt ally needs no mending.
    f.units[1].hp = f.units[1].max_hp;
    assert!(act(&mut f, 0, FightAction::Mend { ally: 1 }, 1).is_err());
    // A healer mends better than a bystander.
    let amount = |trade: Trade| {
        let mut v = vec![
            party("M", trade, 2),
            party("T", Trade::Raker, 1),
            foe("F", 0, 30),
        ];
        v[1].hp = 1;
        v[1].max_hp = 30;
        let mut f = board(v);
        act(&mut f, 0, FightAction::Mend { ally: 1 }, 1).unwrap();
        f.units[1].hp - 1
    };
    assert!(amount(Trade::Healer) > amount(Trade::Raker));
}

#[test]
fn fleeing_is_always_offered_works_more_often_when_quick_and_less_when_overloaded() {
    let rate = |spd: i16, foe_spd: i16, mod_: i16, wet: bool| -> u32 {
        (0..600)
            .filter(|seed| {
                let mut v = vec![party("P", Trade::Fighter, 0), foe("F", 0, 30)];
                v[0].spd = spd;
                v[1].spd = foe_spd;
                let mut f = board(v);
                f.flee_mod = mod_;
                f.wet = wet;
                turn(&mut f, 0);
                let mut lines = Vec::new();
                matches!(
                    f.act(FightAction::Flee, &mut rng(*seed), &mut lines)
                        .unwrap(),
                    Progress::Over(Verdict::Fled)
                )
            })
            .count()
            .try_into()
            .unwrap()
    };
    let quick = rate(9, 3, 0, false);
    let even = rate(5, 5, 0, false);
    let slow = rate(2, 8, 0, false);
    assert!(quick > even && even > slow, "{quick} {even} {slow}");
    assert!(
        rate(5, 5, -20, false) + 100 < even,
        "a heavy pack cannot run"
    );
    assert!(rate(5, 5, 0, true) < even, "mud is slow");
    assert!(slow >= 600 * 15 / 100 - 40, "never impossible: {slow}");
    assert!(quick <= 600 * 90 / 100 + 40, "never certain: {quick}");
}

#[test]
fn water_and_tide_change_the_fight() {
    let mut v = vec![
        party("P", Trade::Fighter, 0),
        foe("Eel", 0, 20),
        foe("Hag", 1, 20),
    ];
    v[1].aquatic = true;
    v[2].tidal = true;
    let mut f = board(v);
    let dry = (
        f.atk_of(&f.units[0]),
        f.atk_of(&f.units[1]),
        f.atk_of(&f.units[2]),
    );
    f.wet = true;
    assert_eq!(
        f.atk_of(&f.units[0]),
        dry.0 - 1,
        "knee-deep is worse for everyone"
    );
    assert_eq!(f.atk_of(&f.units[1]), dry.1 + 1, "and better for the eel");
    assert_eq!(f.def_of(&f.units[1]), f.units[1].def + 1);
    f.level = TIDAL_LEVEL;
    assert_eq!(
        f.atk_of(&f.units[2]),
        dry.2 - 1 + 2,
        "the high water makes the hag stronger"
    );
    // A rising tide ends the fight: the party must leave when the water takes the field.
    let mut f = board(vec![party("P", Trade::Fighter, 0), foe("Wall", 0, 400)]);
    f.limit = 3;
    f.round = 0;
    let mut verdict = None;
    let mut r = rng(4);
    let mut lines = Vec::new();
    let mut p = f.advance(&mut r, &mut lines);
    for _ in 0..40 {
        match p {
            Progress::Over(v) => {
                verdict = Some(v);
                break;
            }
            Progress::Turn(_) => p = f.act(FightAction::Brace, &mut r, &mut lines).unwrap(),
        }
    }
    assert_eq!(verdict, Some(Verdict::Washed));
    assert_eq!(f.round, 3);
    // With no tide, a stalemate ends it at the round limit instead.
    let mut f = board(vec![party("P", Trade::Fighter, 0), foe("Wall", 0, 4000)]);
    f.units[1].atk = 0;
    f.round = 0;
    let mut p = f.advance(&mut r, &mut lines);
    let mut verdict = None;
    for _ in 0..80 {
        match p {
            Progress::Over(v) => {
                verdict = Some(v);
                break;
            }
            Progress::Turn(_) => p = f.act(FightAction::Brace, &mut r, &mut lines).unwrap(),
        }
    }
    assert_eq!(verdict, Some(Verdict::Stalemate));
    assert_eq!(f.round, MAX_ROUNDS);
}

#[test]
fn foes_fight_in_their_own_ways() {
    // A hurler throws from the back at the weakest, whatever the ranks.
    let mut hurler = foe("Slinger", 2, 9);
    hurler.behaviour = Behaviour::Hurler;
    hurler.atk = 8;
    let mut v = vec![
        party("Tank", Trade::Fighter, 0),
        party("Frail", Trade::Healer, 2),
        hurler,
    ];
    v[1].hp = 5;
    v[1].max_hp = 20;
    let mut f = board(v);
    let mut hit_frail = 0;
    for seed in 0..30 {
        f.units[1].hp = 5;
        turn(&mut f, 2);
        f.round = 1;
        f.foe_turn(2, &mut rng(seed), &mut Vec::new());
        hit_frail += i32::from(f.units[1].hp < 5);
    }
    assert!(hit_frail > 20);
    // A lurker goes for the weakest in any rank; a bruiser can only reach the front.
    let mut lurker = foe("Eel", 1, 9);
    lurker.behaviour = Behaviour::Lurker;
    lurker.atk = 8;
    let mut v = vec![
        party("Tank", Trade::Fighter, 0),
        party("Frail", Trade::Healer, 2),
        lurker,
    ];
    v[1].hp = 5;
    let mut f = board(v);
    let mut frail = 0;
    for seed in 0..30 {
        f.units[1].hp = 5;
        f.foe_turn(2, &mut rng(seed), &mut Vec::new());
        frail += i32::from(f.units[1].hp < 5);
    }
    assert!(frail > 20);
    let mut bruiser = foe("Cutter", 0, 20);
    bruiser.atk = 8;
    let mut f = board(vec![
        party("Tank", Trade::Fighter, 0),
        party("Frail", Trade::Healer, 2),
        bruiser,
    ]);
    for seed in 0..30 {
        f.units[0].hp = 20;
        f.units[1].hp = 5;
        f.foe_turn(2, &mut rng(seed), &mut Vec::new());
        assert_eq!(
            f.units[1].hp, 5,
            "the back rank is out of a bruiser's reach"
        );
    }
    // A bruiser in the second rank presses forward before it strikes.
    let mut second = foe("Second", 1, 20);
    second.row = 1;
    let mut f = board(vec![
        party("Tank", Trade::Fighter, 0),
        foe("Lead", 0, 20),
        second,
    ]);
    f.units[2].row = 1;
    let mut lines = Vec::new();
    f.foe_turn(2, &mut rng(1), &mut lines);
    assert!(lines[0].contains("presses forward"), "{lines:?}");
    assert_eq!(f.units[2].row, 0);
    // A skittish foe runs when hurt, and leaves nothing behind (it is gone, not fallen).
    let mut cat = foe("Cat", 0, 10);
    cat.behaviour = Behaviour::Skittish;
    cat.hp = 3;
    let mut f = board(vec![
        party("P", Trade::Fighter, 0),
        cat,
        foe("Other", 0, 20),
    ]);
    let mut lines = Vec::new();
    f.foe_turn(1, &mut rng(1), &mut lines);
    assert!(f.units[1].gone && !f.units[1].downed() && lines[0].contains("bolts"));
}

type Check = fn(&Unit, &Unit) -> bool;

#[test]
fn a_crew_members_numbers_come_from_their_build() {
    let cat = cat();
    let mut c = Crew::generate(&cat, &mut rng(2), 0, Some(Trade::Fighter));
    c.id = 3;
    c.traits.clear();
    c.attrs = [6, 6, 6];
    c.xp = [0; crate::types::TRADES];
    let base = Unit::from_crew(&cat, &c, false);
    assert_eq!((base.row, base.trade), (0, Some(Trade::Fighter)));
    assert!(
        Unit::from_crew(&cat, &c, true).atk == base.atk + 2,
        "arms add to the swing"
    );
    let mut skilled = c.clone();
    skilled.xp[Trade::Fighter.index()] = 100;
    let s = Unit::from_crew(&cat, &skilled, false);
    assert!(
        s.atk > base.atk && s.hp > base.hp,
        "a fighter's skill is felt"
    );
    let mut hurt = c.clone();
    hurt.injuries.push(Injured {
        id: "broken-arm".into(),
        days: 5,
    });
    let h = Unit::from_crew(&cat, &hurt, false);
    assert!(
        h.atk < base.atk && h.hp < base.hp,
        "a broken arm costs both"
    );
    let checks: [(&str, Check); 6] = [
        ("steady", |a, b| a.def == b.def + 1),
        ("limp", |a, b| a.spd == b.spd - 2),
        ("hardy", |a, b| a.hp == b.hp + 3),
        ("shaky-hands", |a, b| a.atk < b.atk),
        ("toughened", |a, b| a.hp > b.hp && a.def > b.def),
        ("sea-born", |a, b| a.aquatic && !b.aquatic && a.spd > b.spd),
    ];
    for (trait_id, check) in checks {
        let mut t = c.clone();
        t.traits.push(trait_id.to_owned());
        assert!(
            check(&Unit::from_crew(&cat, &t, false), &base),
            "{trait_id}"
        );
    }
    // Rows follow the trade.
    for (trade, row) in [
        (Trade::Smith, 0),
        (Trade::Boatwright, 0),
        (Trade::Wader, 1),
        (Trade::Raker, 1),
        (Trade::Salvager, 1),
        (Trade::Healer, 2),
        (Trade::ReedWitch, 2),
    ] {
        let mut t = c.clone();
        t.trade = trade;
        assert_eq!(Unit::from_crew(&cat, &t, false).row, row, "{trade:?}");
    }
}

fn crew_party(cat: &Catalog, seed: u64, size: usize, quality: u8) -> Vec<Unit> {
    let mut r = rng(seed);
    (0..size)
        .map(|i| {
            let mut c = Crew::generate(cat, &mut r, quality, None);
            c.id = u16::try_from(i + 1).unwrap();
            let fighter = i == 0;
            if fighter {
                c.trade = Trade::Fighter;
            }
            Unit::from_crew(cat, &c, fighter && quality > 1)
        })
        .collect()
}

fn run_auto(f: &mut Fight, r: &mut DoorRng, lines: &mut Vec<String>) -> Verdict {
    let mut p = f.advance(r, lines);
    for _ in 0..400 {
        match p {
            Progress::Over(v) => return v,
            Progress::Turn(i) => {
                let a = f.auto(i);
                p = f
                    .act(a, r, lines)
                    .or_else(|_| f.act(FightAction::Brace, r, lines))
                    .unwrap();
            }
        }
        check(f);
    }
    panic!("a fight that never ends");
}

fn check(f: &Fight) {
    for u in &f.units {
        assert!(u.hp <= u.max_hp, "{} has {}/{}", u.name, u.hp, u.max_hp);
        assert!(u.row <= 2 && u.cool <= SPECIAL_COOL + 1);
        assert!(u.max_hp >= 1 && u.atk >= 0);
        if u.gone {
            assert_eq!(u.side, Side::Foe, "only foes run or are swept away");
        }
    }
    assert!(f.round <= MAX_ROUNDS);
    assert!(f.order.iter().all(|o| usize::from(*o) < f.units.len()));
    let mut seen = f.order.clone();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen.len(), f.order.len(), "each unit acts once a round");
}

#[test]
fn any_fight_ends_within_the_round_limit_with_a_verdict_the_board_agrees_with() {
    let cat = cat();
    let mut counts = [0u32; 5];
    for seed in 0..300u64 {
        let g = &cat.groups[usize::try_from(seed).unwrap() % cat.groups.len()];
        let party = crew_party(
            &cat,
            seed,
            1 + usize::try_from(seed % 4).unwrap(),
            u8::try_from(seed % 3).unwrap(),
        );
        let mut f = Fight::begin(
            &cat,
            party,
            &g.id,
            seed % 2 == 0,
            40 + i32::try_from(seed % 50).unwrap(),
            if seed % 7 == 0 { 6 } else { 0 },
            if seed % 5 == 0 { -20 } else { 0 },
        );
        let mut lines = Vec::new();
        let v = run_auto(&mut f, &mut rng(seed), &mut lines);
        counts[match v {
            Verdict::Won => 0,
            Verdict::Lost => 1,
            Verdict::Fled => 2,
            Verdict::Washed => 3,
            Verdict::Stalemate => 4,
        }] += 1;
        match v {
            Verdict::Won => assert!(f.alive(Side::Foe).next().is_none()),
            Verdict::Lost => assert!(f.alive(Side::Party).next().is_none()),
            Verdict::Fled => assert!(f.alive(Side::Party).next().is_some()),
            Verdict::Washed => assert!(f.limit > 0 && f.round >= f.limit),
            Verdict::Stalemate => assert_eq!(f.round, MAX_ROUNDS),
        }
        assert!(lines.len() < 400);
    }
    assert!(
        counts[0] > 50 && counts[1] > 10 && counts[2] > 5,
        "{counts:?}"
    );
}

#[test]
fn the_odds_read_matches_how_quick_resolve_goes() {
    let cat = cat();
    let mut tally = [[0u32; 2]; 4];
    for seed in 0..1200u64 {
        let g = &cat.groups[usize::try_from(seed).unwrap() % cat.groups.len()];
        let party = crew_party(
            &cat,
            seed,
            1 + usize::try_from(seed % 4).unwrap(),
            u8::try_from(seed % 3).unwrap(),
        );
        let mut f = Fight::begin(&cat, party, &g.id, false, 40, 0, 0);
        let odds = f.odds();
        let v = run_auto(&mut f, &mut rng(seed), &mut Vec::new());
        let i = match odds {
            Odds::Easy => 0,
            Odds::Fair => 1,
            Odds::Hard => 2,
            Odds::Grim => 3,
        };
        tally[i][0] += 1;
        tally[i][1] += u32::from(v == Verdict::Won);
    }
    let rate = |i: usize| f64::from(tally[i][1]) / f64::from(tally[i][0].max(1));
    assert!(
        tally.iter().all(|t| t[0] >= 40),
        "every class occurs: {tally:?}"
    );
    assert!(rate(0) > 0.9, "easy: {}", rate(0));
    assert!(
        rate(0) > rate(1) && rate(1) > rate(2) && rate(2) > rate(3),
        "{tally:?}"
    );
    assert!((0.2..0.8).contains(&rate(2)), "a toss-up: {}", rate(2));
    assert!(rate(3) < 0.2, "grim: {}", rate(3));
}

#[test]
fn quick_resolve_is_repeatable_for_a_seed() {
    let cat = cat();
    let run = |seed| {
        let party = crew_party(&cat, 9, 3, 1);
        let mut f = Fight::begin(&cat, party, "cutters-road", false, 40, 0, 0);
        let mut lines = Vec::new();
        let v = run_auto(&mut f, &mut rng(seed), &mut lines);
        (v, lines, f)
    };
    assert_eq!(run(5), run(5));
    assert_ne!(run(5).1, run(6).1);
}

#[test]
fn a_fight_saves_and_loads_midway() {
    let cat = cat();
    let mut f = Fight::begin(
        &cat,
        crew_party(&cat, 3, 3, 1),
        "hound-pack",
        true,
        66,
        8,
        -20,
    );
    let mut r = rng(8);
    let mut lines = Vec::new();
    let Progress::Turn(i) = f.advance(&mut r, &mut lines) else {
        panic!("a turn");
    };
    f.act(FightAction::Brace, &mut r, &mut lines).unwrap();
    f.units[i].netted = 2;
    let mut w = Writer::new();
    f.write(&mut w);
    let bytes = w.finish();
    assert_eq!(Fight::read(&mut Reader::new(&bytes)), Some(f.clone()));
    assert_eq!(
        Fight::read(&mut Reader::new(&bytes[..bytes.len() - 1])),
        None
    );
    // A fight with an order that points off the board is refused.
    let mut bad = f;
    bad.order.push(99);
    let mut w = Writer::new();
    bad.write(&mut w);
    assert_eq!(Fight::read(&mut Reader::new(&w.finish())), None);
}
