//! The headless 30-day simulation: the balance numbers for the PR. The bot plays the engine
//! through the same API the screen uses, sharing one world, buying what a sensible player buys.
#![allow(clippy::unwrap_used, clippy::panic, clippy::cast_precision_loss)]

use russh_bbs_door_nullroute::autoplay::{Config, simulate};
use russh_bbs_door_nullroute::content::Content;

fn mean(v: &[u32]) -> f64 {
    v.iter().map(|x| f64::from(*x)).sum::<f64>() / v.len().max(1) as f64
}

#[test]
fn thirty_days_with_twelve_bots_gives_sane_numbers() {
    let content = Content::try_embedded().unwrap();
    let (mut runs, mut done, mut burned) = (0, 0, 0);
    let (mut fights, mut fragments, mut coded, mut degraded, mut won) = (0, 0, 0, 0, 0);
    for seed in [1u64, 2, 3] {
        let r = simulate(
            &content,
            Config::solo(12, 30, seed, 1),
        );
        let per_player_day = r.earned as f64 / 12.0 / 30.0;
        eprintln!(
            "seed {seed}: runs {} done {} burned {} dry {} | t1 {:?} t2 {:?} t3 {:?} | earned {} ({per_player_day:.0}/player/day) | final marks {:.0} heat {:.0} rep {:.0} rig {:.1}/15 bots {:.1} scripts {:.1} | breaches {} board {} idle days {} | runs reaching trace 60+: {} of {}",
            r.runs(),
            r.done(),
            r.burned(),
            r.dry,
            r.tiers[0],
            r.tiers[1],
            r.tiers[2],
            r.earned,
            mean(&r.marks),
            mean(&r.heat),
            mean(&r.rep),
            mean(&r.rig),
            mean(&r.bots),
            mean(&r.owned),
            r.breaches,
            r.board,
            r.idle_days,
            r.peaks.iter().filter(|p| **p >= 60).count(),
            r.peaks.len()
        );
        eprintln!(
            "  N2: fights {} won {} broke off {} | fragments {} coded {} | degraded decks {} of {} runs, patched rows at the end {} | wardens woke {} down {}",
            r.fights,
            r.defeated,
            r.broke_off,
            r.fragments,
            r.coded,
            r.degraded_decks,
            r.runs(),
            r.patched_rows,
            r.wardens_woke,
            r.wardens_down
        );
        // Money comes in at a pace that buys a rig over a month, not in a day and not never.
        assert!((60.0..220.0).contains(&per_player_day), "{per_player_day}");
        assert!(mean(&r.rig) >= 3.0, "the bots upgraded their rigs");
        assert!(
            r.burned() > 0 || r.runs() < 100,
            "carelessness gets you burned"
        );
        assert!(
            mean(&r.heat) < 90.0,
            "heat does not pin everyone at the ceiling"
        );
        assert!(r.board > 0);
        assert_eq!(r.per_day.len(), 30);
        runs += r.runs();
        fights += r.fights;
        won += r.defeated;
        fragments += r.fragments;
        coded += r.coded;
        degraded += r.degraded_decks;
        done += r.done();
        burned += r.burned();
    }
    // N2: the new systems are all in play, not just present. Fights are common and mostly won,
    // runs bring fragments out, bots code scripts with them, and corporations learn enough that
    // some decks are known (the bots then pick other scripts).
    assert!(fights > 300, "{fights}");
    assert!(won * 100 / fights >= 80, "{won} of {fights} fights won");
    assert!(fragments > 100, "{fragments}");
    assert!(coded > 5, "{coded}");
    assert!(degraded > 50, "{degraded}");
    // Most runs finish the job; some are burned; the rest come away empty-handed.
    assert!(done * 100 / runs >= 55, "{done} of {runs}");
    assert!(burned * 100 / runs <= 10, "{burned} of {runs}");
}

#[test]
fn the_simulation_is_deterministic() {
    let content = Content::try_embedded().unwrap();
    let a = simulate(
        &content,
        Config::solo(4, 10, 9, 1),
    );
    let b = simulate(
        &content,
        Config::solo(4, 10, 9, 1),
    );
    assert_eq!(
        (a.earned, a.runs(), a.marks, a.per_day),
        (b.earned, b.runs(), b.marks, b.per_day)
    );
}
