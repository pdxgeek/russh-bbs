//! Writes the pupil the M4-H5 ssh walk starts its mystery stop with: a pupil who has found the first
//! seven clues, holds the tunnel map and stands in the grounds in a free period, one search from the
//! East Wing's key. Run by `docs/qa/m4-h5-screens/walk.sh`, never by `cargo test`:
//!
//! `HALLPASS_WALK_DAY=<epoch day> HALLPASS_WALK_OUT=<file> cargo test -p russh-bbs-door-hallpass --test walk_save -- --ignored`
//!
//! The file holds the save as hex, for `sqlite3 ... "UPDATE door_save SET data = x'...'"`. Said
//! plainly in the walk's report: this one pupil is prepared; every key after the sign-in is real.
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

use common::Sandbox;
use russh_bbs_door_hallpass::save::encode_player;
use russh_bbs_door_hallpass::state::{ArcProgress, map_set};

#[test]
#[ignore = "writes the ssh walk's prepared pupil; run by docs/qa/m4-h5-screens/walk.sh"]
fn write_the_prepared_pupil_for_the_ssh_walk() {
    let day: i64 = std::env::var("HALLPASS_WALK_DAY")
        .expect("HALLPASS_WALK_DAY")
        .parse()
        .unwrap();
    let out = std::env::var("HALLPASS_WALK_OUT").expect("HALLPASS_WALK_OUT");
    let mut sb = Sandbox::on("smooth_talker", 3, day);
    let p = &mut sb.game.player;
    p.name = "founder".into();
    p.life.arcs.retain(|a| a.id != "the_wing");
    p.life.arcs.push(ArcProgress {
        id: "the_wing".into(),
        step: "key".into(),
        since: day - 1,
        ended: None,
    });
    for clue in [
        "c1_register",
        "c2_hymn",
        "c3_arrows",
        "c4_pipes",
        "c5_loft",
        "c6_tunnel",
        "c7_trunk",
    ] {
        p.learn(&format!("clue_{clue}"));
    }
    for flag in ["organ_key_spot", "ines_locket"] {
        p.learn(flag);
    }
    map_set(&mut p.inventory, "tunnel_map", 1);
    map_set(&mut p.xp, "sneak", 40);
    p.cool = 60;
    p.place = "grounds".into();
    let free = (0..sb.game.periods())
        .find(|s| sb.game.kind(day, *s) == "free")
        .expect("a free period");
    sb.game.player.slot = u8::try_from(free).unwrap();
    let mut hex = String::new();
    for b in encode_player(&sb.game.player) {
        use std::fmt::Write as _;
        write!(hex, "{b:02x}").unwrap();
    }
    std::fs::write(out, hex).unwrap();
}
