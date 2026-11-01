//! Rebuild when a migration is added, because `sqlx::migrate!` embeds the directory.
#![allow(clippy::print_stdout)]

fn main() {
    println!("cargo:rerun-if-changed=migrations");
}
