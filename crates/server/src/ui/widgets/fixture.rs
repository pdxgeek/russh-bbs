//! Sample data for the render tests: every widget has something to show, so the golden screens
//! and the matrix tests draw the real widgets rather than empty cells.
#![allow(clippy::unwrap_used)]

use russh_bbs_core::store::UnixSeconds;
use russh_bbs_core::widgets::{
    Caller, Credits, DoorLine, Headline, Line, MailCount, NewPosts, PollView, RoomCount, Status,
    WidgetData,
};

use super::WidgetSettings;
use crate::config::WidgetsConfig;

/// 2026-11-05 12:00:00 UTC, a Thursday.
pub(crate) const NOW: i64 = 1_793_880_000;

fn at(ago: i64) -> UnixSeconds {
    UnixSeconds::from_raw(NOW - ago)
}

fn line(author: &str, text: &str, ago: i64) -> Line {
    Line {
        author: author.to_owned(),
        text: text.to_owned(),
        at: at(ago),
    }
}

/// Data for every widget.
pub(crate) fn data() -> WidgetData {
    WidgetData {
        at: Some(at(0)),
        online: Some(
            ["alice", "bob", "carol", "dave", "erin"]
                .map(str::to_owned)
                .to_vec(),
        ),
        callers: Some(
            [("bob", 120), ("carol", 3_700), ("dave", 90_000), ("erin", 200_000)]
                .into_iter()
                .map(|(handle, ago)| Caller {
                    handle: handle.to_owned(),
                    at: at(ago),
                })
                .collect(),
        ),
        new_posts: Some(NewPosts {
            threads: 7,
            boards: vec![("General".into(), 4), ("Tech".into(), 2), ("Chat".into(), 1)],
        }),
        mail: Some(MailCount {
            total: 12,
            unread: 3,
        }),
        chat: Some(vec![
            RoomCount {
                name: "lobby".into(),
                members: 3,
            },
            RoomCount {
                name: "tech".into(),
                members: 1,
            },
        ]),
        activity: Some(vec![1, 3, 2, 5, 4, 0, 2, 6, 8, 5, 3, 4, 7, 9]),
        credits: Some(Credits {
            balance: 1_234,
            series: vec![900, 900, 950, 950, 1_000, 1_000, 1_050, 1_100, 1_100, 1_150, 1_150, 1_200, 1_200, 1_234],
        }),
        oneliners: Some(vec![
            line("bob", "the board looks great tonight", 300),
            line("carol", "hello from the terminal", 4_000),
            line("dave", "anyone up for a door game?", 9_000),
        ]),
        wall: Some(vec![
            line("erin", "was here", 10_000),
            line("alice", "russh-bbs forever", 50_000),
        ]),
        poll: Some(PollView {
            question: "Best terminal font?".into(),
            options: vec![
                ("Mono".into(), Some(12)),
                ("Nerd".into(), Some(7)),
                ("Bitmap".into(), Some(3)),
            ],
            my_vote: Some(1),
        }),
        news: Some(vec![
            Headline {
                board: "Rust Blog".into(),
                subject: "A new release of the compiler".into(),
                at: at(1_000),
            },
            Headline {
                board: "Terminals".into(),
                subject: "Ratatui adds new widgets".into(),
                at: at(40_000),
            },
        ]),
        doors: Some(vec![
            DoorLine {
                name: "Saltmarch".into(),
                season: 3,
            },
            DoorLine {
                name: "Long Haul".into(),
                season: 1,
            },
        ]),
        status: Some(Status {
            sessions: 12,
            members: 9,
            capacity: 256,
            frames: 48_210,
            bytes: 9_400_000,
            uptime: 100_000,
        }),
    }
}

/// A message of the day and a quote.
pub(crate) fn settings() -> WidgetSettings {
    WidgetSettings::from_config(&WidgetsConfig {
        motd: "Maintenance on Sunday at 02:00 UTC.".into(),
        quotes: "Be excellent to each other.".into(),
        ..WidgetsConfig::default()
    })
    .unwrap()
}
