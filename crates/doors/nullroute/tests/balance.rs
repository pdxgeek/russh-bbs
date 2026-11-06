//! The N4 balance run: 12 bots, 30 days, three seeds, every system on. The numbers in the PR
//! come from `cargo test -p russh-bbs-door-nullroute --release --test balance -- --nocapture`.
#![allow(clippy::unwrap_used, clippy::panic, clippy::cast_precision_loss)]

use russh_bbs_door_nullroute::autoplay::{Config, Report, TierStats, simulate};
use russh_bbs_door_nullroute::content::Content;

fn pct(a: u32, b: u32) -> String {
    if b == 0 {
        "-".into()
    } else {
        format!("{:.0}%", f64::from(a) * 100.0 / f64::from(b))
    }
}

fn tier_line(name: &str, t: &[TierStats; 3]) -> String {
    t.iter()
        .enumerate()
        .map(|(i, s)| {
            format!(
                "{name} T{}: {} runs, done {} burned {} ({} / {})",
                i + 1,
                s.runs,
                s.done,
                s.burned,
                pct(s.done, s.runs),
                pct(s.burned, s.runs)
            )
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

fn show(seed: u64, r: &Report) {
    eprintln!("== seed {seed}");
    eprintln!("{}", tier_line("solo", &r.tiers));
    eprintln!("{}", tier_line("crew", &r.crew_tiers));
    eprintln!(
        "warden met/tuned/burned/down {:?} | tuned owners {} | dry {} (cycles/trace/stuck {:?}) fights {} defeated {} | wardens woke {:?} down {:?} | first tier 3 day {:?}",
        r.warden_met, r.tuned_owners, r.dry, r.dry_why, r.fights, r.defeated, r.wardens_woke_by_tier, r.wardens_down_by_tier, r.first_tier3_day
    );
    eprintln!(
        "earned {} (solo bots {}, crew bots {}) | coded {} fragments {} | degraded decks {} of {} runs, wary/patched rows by night: first {:?} last {:?}",
        r.earned,
        r.solo_earned,
        r.crew_earned,
        r.coded,
        r.fragments,
        r.degraded_decks,
        r.runs(),
        r.pressure.first(),
        r.pressure.last()
    );
    eprintln!("social {:?}", r.social);
    eprintln!("crew runs {} finale {} season ending {}", r.crew_runs, r.finale, r.season_ending);
    eprintln!("arc steps {:?}", r.story.steps);
    eprintln!("endings {:?} epilogues {:?}", r.story.endings, r.story.epilogues);
    eprintln!("earned by bot {:?}", r.earned_by);
    eprintln!("rep {:?}\nheat {:?}\nmarks {:?}\nrig {:?}\nowned {:?}\nbots {:?}", r.rep, r.heat, r.marks, r.rig, r.owned, r.bots);
}

#[test]
fn the_balance_run() {
    let content = Content::try_embedded().unwrap();
    for seed in [1u64, 2, 3] {
        let r = simulate(&content, Config::full(12, 30, seed, 1));
        show(seed, &r);
    }
}

#[test]
fn a_longer_season() {
    let content = Content::try_embedded().unwrap();
    for seed in [1u64, 2, 3] {
        let r = simulate(&content, Config::full(12, 60, seed, 1));
        eprintln!(
            "60 days seed {seed}: finale {} crew T3 {:?} season ending {} wardens down {:?}",
            r.finale, r.crew_tiers[2], r.season_ending, r.wardens_down_by_tier
        );
    }
}
