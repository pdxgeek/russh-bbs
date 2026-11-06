//! The N4 balance run: 12 bots, 30 days, three seeds, every system on. The numbers in the PR
//! come from `cargo test -p russh-bbs-door-nullroute --release --test balance -- --nocapture`.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::cast_precision_loss,
    clippy::many_single_char_names,
    clippy::assert_is_empty,
    clippy::expect_used
)]

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
        r.warden_met,
        r.tuned_owners,
        r.dry,
        r.dry_why,
        r.fights,
        r.defeated,
        r.wardens_woke_by_tier,
        r.wardens_down_by_tier,
        r.first_tier3_day
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
    eprintln!(
        "crew runs {} finale {} season ending {}",
        r.crew_runs, r.finale, r.season_ending
    );
    eprintln!("arc steps {:?}", r.story.steps);
    eprintln!(
        "endings {:?} epilogues {:?}",
        r.story.endings, r.story.epilogues
    );
    eprintln!("earned by bot {:?}", r.earned_by);
    eprintln!(
        "rep {:?}\nheat {:?}\nmarks {:?}\nrig {:?}\nowned {:?}\nbots {:?}",
        r.rep, r.heat, r.marks, r.rig, r.owned, r.bots
    );
}

#[test]
fn the_balance_run() {
    let content = Content::try_embedded().unwrap();
    let mut endings = std::collections::BTreeSet::new();
    let (mut caught, mut hacks_in, mut bounties, mut filed) = (0, 0, 0, 0);
    let mut honeypots = 0;
    for seed in [1u64, 2, 3, 4] {
        let r = simulate(&content, Config::full(12, 30, seed, 1));
        show(seed, &r);
        let again = simulate(&content, Config::full(12, 30, seed, 1));
        assert_eq!(
            (r.earned, r.runs(), r.crew_runs, r.social.hacks_in),
            (
                again.earned,
                again.runs(),
                again.crew_runs,
                again.social.hacks_in
            ),
            "the same seed gives the same board"
        );
        // Tier 1 is for learning, tier 2 is work, tier 3 is a risk worth taking.
        let t1 = r.tiers[0];
        assert!(
            t1.done * 100 / t1.runs >= 55 && t1.burned * 100 / t1.runs <= 3,
            "{t1:?}"
        );
        let t2 = r.tiers[1];
        assert!((35..=85).contains(&(t2.done * 100 / t2.runs)), "{t2:?}");
        assert!(t2.burned * 100 / t2.runs <= 20, "{t2:?}");
        let t3 = r.tiers[2];
        assert!(t3.runs >= 15, "tier 3 is played: {t3:?}");
        assert!(
            t3.done * 100 / t3.runs >= 25 && t3.burned * 100 / t3.runs <= 35,
            "{t3:?}"
        );
        assert!(
            r.first_tier3_day.is_some_and(|d| d <= 20),
            "{:?}",
            r.first_tier3_day
        );
        // Crews: they run, and they are safer than going alone at the same tier.
        assert!(r.crew_runs >= 30, "{}", r.crew_runs);
        let c2 = r.crew_tiers[1];
        assert!(
            c2.burned * 100 / c2.runs.max(1) <= t2.burned * 100 / t2.runs,
            "{c2:?} {t2:?}"
        );
        assert!(
            c2.done * 100 / c2.runs.max(1) + 15 >= t2.done * 100 / t2.runs,
            "{c2:?} {t2:?}"
        );
        // The pay per bot is in the same range, so a crew is a choice and not a trap.
        let (crew, solo) = (r.crew_earned as f64 / 6.0, r.solo_earned as f64 / 6.0);
        assert!(
            crew > solo * 0.6 && crew < solo * 1.6,
            "crew {crew:.0} solo {solo:.0}"
        );
        // The opt-out holds: nobody hacked the bot that turned the board off, after it did.
        assert_eq!(r.social.hits_on_opted_out, 0);
        endings.extend(r.story.endings.keys().copied());
        caught += r.social.hacks_caught;
        hacks_in += r.social.hacks_in;
        bounties += r.social.bounties_paid;
        filed += r.social.hacks_filed;
        honeypots += r.social.honeypot_owners;
        // Every bot got an epilogue, and the season closed into the hall.
        assert_eq!(r.story.epilogues.values().sum::<u32>(), 12);
        assert!(r.season_ending != 0);
        // The economy: a rig is a month's work, the market moves, the corporations learn.
        assert!(r.market.iter().any(|m| *m != 100) && r.market.len() == 30);
        assert!(r.pressure.last().copied().unwrap_or(0) > 0 || r.degraded_decks > 0);
    }
    assert!(
        filed > 40 && hacks_in > 20,
        "hacks happen: {filed} filed, {hacks_in} in"
    );
    assert!(caught > 5, "honeypots catch: {caught}");
    assert!(honeypots >= 8, "bots set honeypots: {honeypots}");
    assert!(bounties > 3, "bounties are paid: {bounties}");
    assert!(
        endings.len() >= 9,
        "endings reached by the bots across four seeds: {endings:?}"
    );
}

#[test]
fn the_warden_gate() {
    use russh_bbs_door_nullroute::autoplay::warden_trial;
    let content = Content::try_embedded().unwrap();
    let stock = warden_trial(&content, 5, 120, false);
    let ready = warden_trial(&content, 5, 120, true);
    eprintln!("warden trial, stock deck: {stock:?}");
    eprintln!("warden trial, prepared:   {ready:?}");
    // The stock deck cannot put one down; a character ready for it does, about half the times it
    // is met, and not without risk.
    assert_eq!(stock.down, 0, "{stock:?}");
    assert!(
        stock.met < stock.woke,
        "most players leave before it arrives"
    );
    assert!(
        ready.met >= 40 && ready.down * 100 / ready.met >= 35,
        "{ready:?}"
    );
    assert!(ready.burned > 0, "{ready:?}");
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

/// The branch coverage report for the PR: which steps, choices and endings twelve seeds of
/// bots reached. `cargo test -p russh-bbs-door-nullroute --release --test balance branch_coverage -- --ignored --nocapture`.
#[test]
#[ignore = "a report, not a check: run it in release"]
fn branch_coverage() {
    use std::collections::BTreeMap;
    let content = Content::try_embedded().unwrap();
    let mut steps: BTreeMap<(u8, u8), u32> = BTreeMap::new();
    let mut choices: BTreeMap<(u8, u8, usize), u32> = BTreeMap::new();
    let mut endings: BTreeMap<u8, u32> = BTreeMap::new();
    let mut epilogues: BTreeMap<u8, u32> = BTreeMap::new();
    let mut season_endings: BTreeMap<u8, u32> = BTreeMap::new();
    let (mut finales, mut seeds) = (0, 0);
    for seed in 1..=12u64 {
        let r = simulate(&content, Config::full(12, 30, seed, 1));
        for (k, v) in r.story.steps {
            *steps.entry(k).or_insert(0) += v;
        }
        for (k, v) in r.story.choices {
            *choices.entry(k).or_insert(0) += v;
        }
        for (k, v) in r.story.endings {
            *endings.entry(k).or_insert(0) += v;
        }
        for (k, v) in r.story.epilogues {
            *epilogues.entry(k).or_insert(0) += v;
        }
        *season_endings.entry(r.season_ending).or_insert(0) += 1;
        finales += u32::from(r.finale);
        seeds += 1;
    }
    let all_steps = content.story.step.len();
    let all_choices: usize = content.story.step.iter().map(|s| s.choice.len()).sum();
    eprintln!("{seeds} seeds x 12 bots x 30 days");
    eprintln!("steps done {}/{}: {steps:?}", steps.len(), all_steps);
    eprintln!(
        "choices made {}/{}: {choices:?}",
        choices.len(),
        all_choices
    );
    eprintln!(
        "endings {}/{}: {endings:?}",
        endings.len(),
        content.story.ending.len()
    );
    eprintln!(
        "epilogues {}/{}: {epilogues:?}",
        epilogues.len(),
        content.seasons.epilogue.len()
    );
    eprintln!("season endings: {season_endings:?}; finale cracked in {finales} seasons");
}

#[test]
fn a_crew_that_is_ready_can_crack_the_finale() {
    use russh_bbs_door_nullroute::autoplay::finale_trial;
    let content = Content::try_embedded().unwrap();
    let cracked = finale_trial(&content, 9, 16);
    eprintln!("finale: cracked in {cracked} of 16 seasons by a ready crew");
    assert!(cracked >= 4, "{cracked}");
}
