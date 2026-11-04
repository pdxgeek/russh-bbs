#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::type_complexity,
    clippy::panic,
    clippy::assert_is_empty
)]

//! Render snapshots of every style in every layout (UI-2).
//!
//! One golden file per layout in `snapshots/`, holding each style at 80x24 and 132x43 (Unicode
//! tier, truecolor) and at 80x24 in the ASCII tier with colour off. The text is what a terminal
//! shows with the colours dropped, with trailing spaces trimmed. Each screen also carries a
//! `colors=` fingerprint of every cell's colours and weights, so a palette change shows up as a
//! diff too.
//!
//! To accept a deliberate change: `UPDATE_SNAPSHOTS=1 cargo test -p russh-bbs-server snapshots`,
//! then read the diff.

use std::fmt::Write as _;
use std::path::PathBuf;

use ratatui::buffer::Buffer;
use russh_bbs_core::icons::Tier;
use russh_bbs_core::terminal_settings::LayoutId;

use super::depth::ColorDepth;
use super::layout_tests::{text, ui};
use super::styles_tests::{STYLES, shot, style};

/// FNV-1a over every cell's colours and modifiers (stable across Rust versions).
fn fingerprint(buffer: &Buffer) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for cell in &buffer.content {
        let facts = format!("{:?}{:?}{:?}", cell.fg, cell.bg, cell.modifier);
        for byte in facts.bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
    }
    hash
}

fn layout_slug(layout: LayoutId) -> &'static str {
    russh_bbs_core::terminal_settings::layout_to_stored(layout)
}

fn render(layout: LayoutId) -> String {
    let ui = ui();
    let body = vec!["Signed in as alice.".to_owned()];
    let mut out = String::new();
    for name in STYLES {
        let theme = style(&ui, name);
        for (size, tier, color, depth, label) in [
            (
                (80, 24),
                Tier::Unicode,
                true,
                ColorDepth::TrueColor,
                "unicode truecolor",
            ),
            (
                (132, 43),
                Tier::Unicode,
                true,
                ColorDepth::TrueColor,
                "unicode truecolor",
            ),
            ((80, 24), Tier::Ascii, false, ColorDepth::Mono, "ascii mono"),
        ] {
            let buffer = shot(&[], layout, theme, &body, tier, color, depth, size);
            let _ = writeln!(
                out,
                "=== {name} / {} / {}x{} / {label} / colors={:016x}",
                layout_slug(layout),
                size.0,
                size.1,
                fingerprint(&buffer)
            );
            for line in text(&buffer).lines() {
                let _ = writeln!(out, "{}", line.trim_end());
            }
            out.push('\n');
        }
    }
    out
}

fn path(layout: LayoutId) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src/ui/snapshots")
        .join(format!("{}.txt", layout_slug(layout)))
}

fn check(layout: LayoutId) {
    let got = render(layout);
    let file = path(layout);
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, &got).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&file).unwrap_or_else(|_| {
        panic!(
            "{} is missing; run with UPDATE_SNAPSHOTS=1 to create it",
            file.display()
        )
    });
    if got != want {
        let first = got
            .lines()
            .zip(want.lines())
            .position(|(a, b)| a != b)
            .unwrap_or_else(|| got.lines().count().min(want.lines().count()));
        panic!(
            "snapshot {} differs at line {}:\n  got:  {:?}\n  want: {:?}\nrun with UPDATE_SNAPSHOTS=1 to accept",
            file.display(),
            first + 1,
            got.lines().nth(first),
            want.lines().nth(first)
        );
    }
}

#[test]
fn classic_snapshots() {
    check(LayoutId::Classic);
}

#[test]
fn lightbar_snapshots() {
    check(LayoutId::Lightbar);
}

#[test]
fn compact_snapshots() {
    check(LayoutId::Compact);
}

#[test]
fn dashboard_snapshots() {
    check(LayoutId::Dashboard);
}

#[test]
fn wide_snapshots() {
    check(LayoutId::Wide);
}

#[test]
fn focus_snapshots() {
    check(LayoutId::Focus);
}

#[test]
fn split_snapshots() {
    check(LayoutId::Split);
}
