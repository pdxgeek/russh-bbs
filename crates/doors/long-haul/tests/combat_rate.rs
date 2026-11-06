//! Win rates in ship combat, for balance (not a guard rail): `--ignored --nocapture`.
#![allow(
    clippy::unwrap_used,
    clippy::print_stdout,
    missing_docs,
    clippy::panic,
    clippy::many_single_char_names
)]

mod common;

use common::{Harness, orders};
use russh_bbs_door_long_haul::combat::Phase;
use russh_bbs_door_long_haul::crew::Role;
use russh_bbs_door_long_haul::engine::Action;
use russh_bbs_door_long_haul::scene::Scene;

#[test]
#[ignore = "balance report"]
fn win_rates() {
    for enemy in ["skiff", "raider", "corsair", "jumper"] {
        for (label, gunner) in [("lance+shield", false), ("lance+shield+gunner", true)] {
            let (mut won, mut ran, mut lost) = (0, 0, 0);
            for seed in 0..30u64 {
                let mut h = Harness::new(1, seed);
                let mut p = h.captain("dockhand");
                h.fit(&mut p, "lance1");
                h.fit(&mut p, "shield1");
                if gunner {
                    h.sign(&mut p, Role::Gunner, 2);
                }
                h.fight(&mut p, enemy);
                for _ in 0..40 {
                    let Some(Scene::Combat(c)) = p.scene.clone() else {
                        break;
                    };
                    let hull = u32::from(p.ship.hull_hp);
                    let max = u32::from(h.content.hulls[usize::from(p.ship.hull)].hull_points);
                    let a = if c.phase == Phase::Won {
                        Action::Wreck { board: false }
                    } else if hull * 100 < max * 35 {
                        Action::Run
                    } else {
                        let b = p.gear(&h.content).budget;
                        let w = (b * 3).div_ceil(5);
                        Action::Fight(orders(w, b - w, 0))
                    };
                    let _ = h.go(&mut p, &a);
                }
                if p.stats.fights_won > 0 {
                    won += 1;
                } else if p.stats.fled > 0 {
                    ran += 1;
                } else {
                    lost += 1;
                }
            }
            println!("{enemy:<8} {label:<22} won {won:>3} ran {ran:>3} other {lost:>3} of 30");
        }
    }
}
