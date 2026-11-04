//! Combat: telegraphed intent is honored, stances and rows matter, moves do what they say.

use super::support::{content, game_of, hero_of, place, said};
use crate::combat::{FightAct, Stance};
use crate::content::{Intent, Row};
use crate::game::{Action, Event, Game, Place};

fn fight_game(calling: &str, enemies: &[&str], seed: u64) -> Game {
    let c = content();
    let mut g = game_of(calling, ["quick", "stout"], seed);
    place(&mut g, "bone-pit");
    g.hero.hp = 10_000;
    let ids: Vec<String> = enemies.iter().map(|s| (*s).to_owned()).collect();
    let mut ev = Vec::new();
    g.start_fight(&c, &ids, false, &mut ev);
    g
}

fn set_intent(g: &mut Game, i: usize, intent: Intent) {
    g.fight.as_mut().unwrap().enemies[i].intent = intent;
}

fn acted(events: &[Event]) -> Vec<(usize, Intent, i32, bool)> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Acted {
                enemy,
                intent,
                damage,
                dodged,
            } => Some((*enemy, *intent, *damage, *dodged)),
            _ => None,
        })
        .collect()
}

#[test]
fn the_intent_an_enemy_shows_is_exactly_what_it_does() {
    let c = content();
    let packs: [&[&str]; 5] = [
        &["slag-rat", "cinder-imp", "rust-warden"],
        &["slag-crawler", "slag-spitter", "forge-thrall"],
        &["glass-wisp", "mirror-knight", "echo-hound"],
        &["ashen-sentinel", "cinder-priest", "hearth-wyrm"],
        &["bellows-golem", "slag-rat", "cold-furnace"],
    ];
    let callings = [
        "warden",
        "cutthroat",
        "ember-priest",
        "runesmith",
        "wanderer",
        "duelist",
    ];
    let mut rounds_checked = 0;
    for seed in 0..240u64 {
        let calling = callings[usize::try_from(seed).unwrap() % 6];
        let pack = packs[usize::try_from(seed / 6).unwrap() % 5];
        let mut g = fight_game(calling, pack, seed);
        // What the board showed at the start of the fight.
        let mut shown: Vec<(usize, Intent)> = g
            .fight
            .as_ref()
            .unwrap()
            .alive()
            .map(|(i, e)| (i, e.intent))
            .collect();
        for round in 0..14 {
            let Some(fight) = g.fight.as_ref() else {
                break;
            };
            let alive_before: Vec<usize> = fight.alive().map(|(i, _)| i).collect();
            let target = alive_before[0];
            let act = match (round + seed) % 6 {
                0 => FightAct::Stance(Stance::Guarded),
                1 => FightAct::Attack(target),
                2 => FightAct::Step,
                3 => FightAct::Stance(Stance::Evasive),
                4 => FightAct::Attack(target),
                _ => FightAct::Step,
            };
            let free = matches!(act, FightAct::Stance(_));
            let Ok(events) = g.apply(&c, Action::Fight(act)) else {
                continue;
            };
            if free {
                continue;
            }
            let did = acted(&events);
            for (enemy, intent, _, _) in &did {
                assert!(
                    shown.contains(&(*enemy, *intent)),
                    "seed {seed}: enemy {enemy} did {intent:?} but showed {shown:?}"
                );
                rounds_checked += 1;
            }
            let interrupted: Vec<usize> = events
                .iter()
                .filter_map(|e| {
                    if let Event::Interrupted { enemy } = e {
                        Some(*enemy)
                    } else {
                        None
                    }
                })
                .collect();
            if let Some(fight) = g.fight.as_ref() {
                // Every enemy still standing did something, or was stopped: none sat out.
                for (i, _) in fight.alive() {
                    if alive_before.contains(&i) {
                        assert!(
                            did.iter().any(|(e, ..)| *e == i) || interrupted.contains(&i),
                            "seed {seed}: enemy {i} neither acted nor was interrupted"
                        );
                    }
                }
            }
            // The next round's announcements.
            let next: Vec<(usize, Intent)> = events
                .iter()
                .filter_map(|e| {
                    if let Event::Announced { enemy, intent } = e {
                        Some((*enemy, *intent))
                    } else {
                        None
                    }
                })
                .collect();
            if g.fight.is_some() {
                assert!(
                    !next.is_empty(),
                    "seed {seed}: a fight that goes on announces what is next"
                );
                shown = next;
            }
        }
    }
    assert!(
        rounds_checked > 400,
        "only {rounds_checked} enemy actions were checked"
    );
}

#[test]
fn an_announcement_does_not_change_when_the_hero_acts() {
    let c = content();
    let mut g = fight_game("warden", &["rust-warden"], 3);
    let shown = g.fight.as_ref().unwrap().enemies[0].intent;
    // Free actions (stances) never touch the intent.
    for s in [Stance::Guarded, Stance::Evasive, Stance::Aggressive] {
        g.apply(&c, Action::Fight(FightAct::Stance(s))).unwrap();
        assert_eq!(g.fight.as_ref().unwrap().enemies[0].intent, shown);
    }
}

#[test]
fn an_evasive_hero_is_never_caught_by_a_heavy_blow() {
    let c = content();
    for seed in 0..100 {
        let mut g = fight_game("duelist", &["bellows-golem"], seed);
        set_intent(&mut g, 0, Intent::Heavy);
        g.apply(&c, Action::Fight(FightAct::Stance(Stance::Evasive)))
            .unwrap();
        let ev = g.apply(&c, Action::Fight(FightAct::Attack(0))).unwrap();
        let did = acted(&ev);
        assert_eq!(did[0].1, Intent::Heavy);
        assert!(did[0].3 && did[0].2 == 0, "seed {seed}: {did:?}");
    }
}

#[test]
fn stance_changes_what_a_blow_costs() {
    let c = content();
    let (mut aggressive, mut guarded) = (0, 0);
    for seed in 0..200 {
        for (stance, total) in [
            (Stance::Aggressive, &mut aggressive),
            (Stance::Guarded, &mut guarded),
        ] {
            let mut g = fight_game("warden", &["rust-warden"], seed);
            set_intent(&mut g, 0, Intent::Strike);
            g.apply(&c, Action::Fight(FightAct::Stance(stance)))
                .unwrap();
            let ev = g.apply(&c, Action::Fight(FightAct::Attack(0))).unwrap();
            *total += acted(&ev).iter().map(|a| a.2).sum::<i32>();
        }
    }
    assert!(
        aggressive > guarded * 2,
        "aggressive {aggressive} vs guarded {guarded}"
    );
}

#[test]
fn attacking_a_bracing_enemy_wastes_half_the_blow() {
    let c = content();
    let mut sum = (0, 0);
    for seed in 0..60 {
        for (k, intent) in [Intent::Strike, Intent::Brace].into_iter().enumerate() {
            let mut g = fight_game("warden", &["bellows-golem"], seed);
            set_intent(&mut g, 0, intent);
            let before = g.fight.as_ref().unwrap().enemies[0].hp;
            g.apply(&c, Action::Fight(FightAct::Attack(0))).unwrap();
            let lost = before - g.fight.as_ref().unwrap().enemies[0].hp;
            if k == 0 { sum.0 += lost } else { sum.1 += lost }
        }
    }
    assert!(
        sum.1 * 10 < sum.0 * 7,
        "braced took {} vs open {}",
        sum.1,
        sum.0
    );
}

#[test]
fn a_bracing_enemy_does_not_hit_back() {
    let c = content();
    let mut g = fight_game("warden", &["rust-warden"], 9);
    set_intent(&mut g, 0, Intent::Brace);
    let hp = g.hero.hp;
    let ev = g.apply(&c, Action::Fight(FightAct::Attack(0))).unwrap();
    assert_eq!(g.hero.hp, hp);
    assert_eq!(acted(&ev)[0].2, 0);
}

#[test]
fn rows_decide_who_can_be_hit_by_what() {
    let c = content();
    // A melee hero cannot hit the back row until the front is gone.
    let mut g = fight_game("warden", &["rust-warden", "cinder-imp"], 4);
    let err = g.apply(&c, Action::Fight(FightAct::Attack(1))).unwrap_err();
    assert!(err.0.contains("behind"), "{err:?}");
    // A ranged hero can.
    let mut r = fight_game("wanderer", &["rust-warden", "cinder-imp"], 4);
    assert!(r.apply(&c, Action::Fight(FightAct::Attack(1))).is_ok());
    // Stepping back means melee attacks are refused until you step forward.
    let mut b = fight_game("warden", &["rust-warden"], 4);
    b.apply(&c, Action::Fight(FightAct::Step)).unwrap();
    assert_eq!(b.fight.as_ref().unwrap().row, Row::Back);
    let err = b.apply(&c, Action::Fight(FightAct::Attack(0))).unwrap_err();
    assert!(err.0.contains("too far back"), "{err:?}");
    // Killing the front row brings the back row forward.
    let mut k = fight_game("warden", &["rust-warden", "cinder-imp"], 5);
    k.fight.as_mut().unwrap().enemies[0].hp = 1;
    k.apply(&c, Action::Fight(FightAct::Attack(0))).unwrap();
    assert_eq!(k.fight.as_ref().unwrap().enemies[1].row, Row::Front);
}

#[test]
fn a_melee_enemy_must_close_in_on_a_hero_in_the_back_row() {
    let c = content();
    let mut g = fight_game("wanderer", &["rust-warden"], 6);
    g.apply(&c, Action::Fight(FightAct::Step)).unwrap();
    // Round one after stepping back: the enemy's next announcement is to close in, and until it
    // does it cannot reach the hero.
    assert_eq!(g.fight.as_ref().unwrap().enemies[0].intent, Intent::Advance);
    let hp = g.hero.hp;
    let ev = g.apply(&c, Action::Fight(FightAct::Attack(0))).unwrap();
    assert_eq!(acted(&ev)[0].1, Intent::Advance);
    assert_eq!(g.hero.hp, hp, "it spent the round closing in");
    assert!(g.fight.as_ref().unwrap().enemies[0].engaged);
}

#[test]
fn an_interrupt_ruins_the_wind_up_it_was_aimed_at() {
    let c = content();
    for (calling, mv) in [
        ("warden", "shield-bash"),
        ("cutthroat", "cheap-shot"),
        ("wanderer", "snare-shot"),
    ] {
        let mut g = fight_game(calling, &["bellows-golem"], 2);
        set_intent(&mut g, 0, Intent::Heavy);
        let hp = g.hero.hp;
        let ev = g
            .apply(&c, Action::Fight(FightAct::Use(mv.into(), 0)))
            .unwrap();
        assert!(
            ev.contains(&Event::Interrupted { enemy: 0 }),
            "{calling}: {}",
            said(&ev)
        );
        assert!(acted(&ev).is_empty());
        assert_eq!(g.hero.hp, hp, "{calling}: no damage taken");
    }
    // A build without the move simply cannot.
    let mut g = fight_game("runesmith", &["bellows-golem"], 2);
    let err = g
        .apply(&c, Action::Fight(FightAct::Use("shield-bash".into(), 0)))
        .unwrap_err();
    assert!(err.0.contains("do not know"));
}

#[test]
fn moves_cost_focus_and_focus_comes_back_a_round_at_a_time() {
    let c = content();
    let mut g = fight_game("warden", &["bellows-golem"], 8);
    let focus = g.fight.as_ref().unwrap().focus;
    assert_eq!(focus, 1);
    g.apply(&c, Action::Fight(FightAct::Use("hold-the-line".into(), 0)))
        .unwrap_err();
    assert_eq!(
        g.fight.as_ref().unwrap().focus,
        1,
        "a refused move costs nothing"
    );
    g.apply(&c, Action::Fight(FightAct::Use("shield-bash".into(), 0)))
        .unwrap();
    // One spent, one regained at the end of the round.
    assert_eq!(g.fight.as_ref().unwrap().focus, 1);
    g.apply(&c, Action::Fight(FightAct::Attack(0))).unwrap();
    assert_eq!(g.fight.as_ref().unwrap().focus, 2);
}

#[test]
fn marks_sunders_bleeds_and_shoves_all_land() {
    let c = content();
    let mut priest = fight_game("ember-priest", &["bellows-golem"], 1);
    priest.hero.nodes.push("ep-brand".into());
    priest.fight.as_mut().unwrap().focus = 5;
    priest
        .apply(&c, Action::Fight(FightAct::Use("brand".into(), 0)))
        .unwrap();
    assert_eq!(priest.fight.as_ref().unwrap().enemies[0].mark_pct, 30);

    let mut smith = fight_game("runesmith", &["bellows-golem"], 1);
    smith.fight.as_mut().unwrap().focus = 5;
    smith
        .apply(&c, Action::Fight(FightAct::Use("rune-strike".into(), 0)))
        .unwrap();
    assert_eq!(smith.fight.as_ref().unwrap().enemies[0].sunder, 2);

    let mut thief = fight_game("cutthroat", &["bellows-golem"], 1);
    thief.hero.nodes.push("cu-knifework".into());
    thief.fight.as_mut().unwrap().focus = 5;
    let hp = thief.fight.as_ref().unwrap().enemies[0].hp;
    thief
        .apply(&c, Action::Fight(FightAct::Use("open-vein".into(), 0)))
        .unwrap();
    let after = thief.fight.as_ref().unwrap().enemies[0].hp;
    assert!(hp - after >= 3 + 1, "the cut and the bleed");
    assert_eq!(thief.fight.as_ref().unwrap().enemies[0].bleed, 3);

    let mut warden = fight_game("warden", &["rust-warden", "slag-rat"], 1);
    warden.hero.nodes.push("wa-shove".into());
    warden.fight.as_mut().unwrap().focus = 5;
    warden
        .apply(&c, Action::Fight(FightAct::Use("shove-back".into(), 0)))
        .unwrap();
    assert_eq!(warden.fight.as_ref().unwrap().enemies[0].row, Row::Back);
}

#[test]
fn a_ward_soaks_damage_before_armour() {
    let c = content();
    let mut g = fight_game("warden", &["rust-warden"], 12);
    set_intent(&mut g, 0, Intent::Strike);
    g.fight.as_mut().unwrap().focus = 3;
    let hp = g.hero.hp;
    let ev = g
        .apply(&c, Action::Fight(FightAct::Use("hold-the-line".into(), 0)))
        .unwrap();
    assert!(g.fight.as_ref().unwrap().ward < 12, "some of it was used");
    assert_eq!(g.hero.hp, hp, "{}", said(&ev));
}

#[test]
fn quick_resolve_is_cheaper_but_cannot_be_used_on_a_guardian_or_late() {
    let c = content();
    let mut g = fight_game("warden", &["slag-rat"], 5);
    let turns = g.hero.turns;
    g.apply(&c, Action::Fight(FightAct::Quick)).unwrap();
    assert!(g.fight.is_none(), "the fight resolved");
    assert_eq!(g.hero.turns, turns - 1);
    let mut m = fight_game("warden", &["slag-rat"], 5);
    let turns = m.hero.turns;
    m.apply(&c, Action::Fight(FightAct::Attack(0))).unwrap();
    assert_eq!(m.hero.turns, turns - 2, "a played fight costs two turns");
    // Not after the first round.
    let mut late = fight_game("warden", &["bellows-golem"], 5);
    late.apply(&c, Action::Fight(FightAct::Attack(0))).unwrap();
    assert!(late.apply(&c, Action::Fight(FightAct::Quick)).is_err());
    // Not against a guardian.
    let mut guardian = game_of("warden", ["quick", "stout"], 5);
    place(&mut guardian, "bone-pit");
    let mut ev = Vec::new();
    guardian.start_fight(&c, &["bellows-golem".to_owned()], true, &mut ev);
    assert!(guardian.apply(&c, Action::Fight(FightAct::Quick)).is_err());
}

#[test]
fn reading_the_fight_beats_quick_resolve() {
    // The same hero against the same pack: playing it by the intents loses less health than
    // letting it resolve, over many fights.
    let c = content();
    let mut played = 0;
    let mut quick = 0;
    for seed in 0..80 {
        let mut a = fight_game(
            "warden",
            &["rust-warden", "forge-thrall", "slag-spitter"],
            seed,
        );
        a.hero.hp = a.derived(&c).max_hp;
        let start = a.hero.hp;
        for _ in 0..30 {
            let Some(f) = a.fight.as_ref() else { break };
            let heavy = f.alive().any(|(_, e)| e.intent == Intent::Heavy);
            let stance = if heavy {
                Stance::Evasive
            } else {
                Stance::Guarded
            };
            let _ = a.apply(&c, Action::Fight(FightAct::Stance(stance)));
            let f = a.fight.as_ref().unwrap();
            let target = f
                .alive()
                .find(|(_, e)| e.row == Row::Front)
                .map(|(i, _)| i)
                .unwrap_or(0);
            let _ = a.apply(&c, Action::Fight(FightAct::Attack(target)));
        }
        played += start - a.hero.hp;
        let mut b = fight_game(
            "warden",
            &["rust-warden", "forge-thrall", "slag-spitter"],
            seed,
        );
        b.hero.hp = b.derived(&c).max_hp;
        let start = b.hero.hp;
        b.apply(&c, Action::Fight(FightAct::Quick)).unwrap();
        quick += start - b.hero.hp;
    }
    assert!(played < quick, "played {played} vs quick {quick}");
}

#[test]
fn a_win_pays_experience_coin_and_salvage_and_clears_the_room() {
    let c = content();
    let mut g = fight_game("warden", &["slag-rat"], 11);
    g.hero.hp = g.derived(&c).max_hp;
    let (coin, xp) = (g.hero.coin, g.hero.xp);
    for _ in 0..20 {
        if g.fight.is_none() {
            break;
        }
        let _ = g.apply(&c, Action::Fight(FightAct::Attack(0)));
    }
    assert!(g.fight.is_none());
    assert!(g.hero.xp > xp && g.hero.coin > coin);
    let Place::Delve { depth, room } = g.place else {
        panic!("still below")
    };
    assert!(g.today.is_cleared(depth, room));
}

#[test]
fn killing_a_guardian_opens_the_way_for_everyone() {
    let c = content();
    let mut g = fight_game("warden", &["slag-rat"], 1);
    g.fight.as_mut().unwrap().guardian = true;
    // Make the "enemy" the real guardian so the win records it.
    let mut ev = Vec::new();
    g.fight = None;
    g.start_fight(&c, &["bellows-golem".to_owned()], true, &mut ev);
    g.fight.as_mut().unwrap().enemies[0].hp = 1;
    g.apply(&c, Action::Fight(FightAct::Attack(0))).unwrap();
    assert!(g.world.is_slain(1));
    assert!(
        g.take_changes()
            .iter()
            .any(|ch| matches!(ch, crate::world::Change::Slain(1, _)))
    );
    assert!(g.hero.relics >= 1);
}

#[test]
fn fleeing_works_sometimes_and_a_guardian_is_harder_to_leave() {
    let c = content();
    let (mut gone, mut gone_guardian) = (0, 0);
    for seed in 0..300 {
        let mut g = fight_game("duelist", &["slag-rat"], seed);
        g.apply(&c, Action::Fight(FightAct::Flee)).unwrap();
        gone += usize::from(g.fight.is_none());
        let mut h = fight_game("duelist", &["bellows-golem"], seed);
        h.fight.as_mut().unwrap().guardian = true;
        h.apply(&c, Action::Fight(FightAct::Flee)).unwrap();
        gone_guardian += usize::from(h.fight.is_none());
    }
    assert!(gone > 100 && gone < 290, "{gone}");
    assert!(gone_guardian < gone, "{gone_guardian} vs {gone}");
}

#[test]
fn a_fight_cannot_be_walked_away_from() {
    let c = content();
    let mut g = fight_game("warden", &["slag-rat"], 2);
    let err = g.apply(&c, Action::Climb).unwrap_err();
    assert!(err.0.contains("fight"));
    let _ = hero_of("warden", ["quick", "stout"]);
}
