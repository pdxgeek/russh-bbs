//! Prints a rescue pair as hex for seeding a scratch board: `rescue_pair TODAY AGO FALLEN RESCUER`.
use russh_bbs_door_emberdeep::showcase::rescue_pair_at;

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let today: i64 = a[0].parse().unwrap_or(0);
    let ago: i64 = a[1].parse().unwrap_or(1);
    let p = rescue_pair_at(true, today, ago, &a[2], &a[3]);
    println!("{}\n{}\n{}", hex(&p.fallen), hex(&p.rescuer), hex(&p.world));
}
