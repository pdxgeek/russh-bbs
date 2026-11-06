//! Prints a rescue pair as hex for seeding a scratch board: `rescue_pair TODAY AGO FALLEN RESCUER`.
#![allow(clippy::print_stdout, reason = "an example whose output is the point")]

use std::fmt::Write as _;

use russh_bbs_door_emberdeep::showcase::rescue_pair_at;

fn hex(b: &[u8]) -> String {
    b.iter().fold(String::new(), |mut out, x| {
        let _ = write!(out, "{x:02x}");
        out
    })
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let today: i64 = a.first().and_then(|s| s.parse().ok()).unwrap_or(0);
    let ago: i64 = a.get(1).and_then(|s| s.parse().ok()).unwrap_or(1);
    let fallen = a.get(2).map_or("founder", String::as_str);
    let rescuer = a.get(3).map_or("bob", String::as_str);
    let p = rescue_pair_at(true, today, ago, fallen, rescuer);
    println!("{}\n{}\n{}", hex(&p.fallen), hex(&p.rescuer), hex(&p.world));
}
