//! H-4: the server config file under hostile input. The file is the sysop's, but it is parsed
//! at boot and must refuse a bad one with a single clear reason, never panic, and never put a
//! password in a message.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fmt::Write as _;
use std::path::Path;
use std::sync::OnceLock;

use proptest::prelude::*;

use super::keys::{self, KEYS};
use super::{ConfigError, Dirs, FileInput, Loaded, assemble};
use crate::fuzz::{garbage, mutated, random_text, reason_is_loggable};

const FILE: &str = "/etc/bbs/config.toml";
/// Keys left out of the all-keys sample: each is valid alone but not beside the others.
const SKIP: &[&str] = &[
    "blobs.ca_file",
    "blobs.dir",
    "datasource.tls_root_cert",
    "datasource.path",
    "feeds.ca_file",
    "board.layout",
    "board.style",
];
const CANARY: &str = "CANARYPASSWORD7";

/// A document with every key set to its sample value, except the database password, which is a
/// canary the messages must never repeat.
fn sample() -> &'static str {
    static TEXT: OnceLock<String> = OnceLock::new();
    TEXT.get_or_init(|| {
        // Top-level keys first (a key after a table header would belong to the table).
        let mut sections: std::collections::BTreeMap<&str, Vec<String>> =
            std::collections::BTreeMap::new();
        for def in KEYS {
            // Samples that contradict each other when all are set at once.
            if SKIP.contains(&def.path) {
                continue;
            }
            let value = match def.kind {
                keys::Kind::Str => format!("{:?}", def.sample),
                keys::Kind::Int | keys::Kind::Bool => def.sample.to_owned(),
            };
            let (section, leaf) = keys::split(def.path);
            sections
                .entry(section.unwrap_or(""))
                .or_default()
                .push(format!("{leaf} = {value}"));
        }
        let mut text = String::new();
        for (section, lines) in &sections {
            if !section.is_empty() {
                let _ = writeln!(text, "[{section}]");
            }
            for line in lines {
                text.push_str(line);
                text.push('\n');
            }
        }
        let text = text.replace("kind = \"sqlite\"", "kind = \"postgres\"");
        // Put the canary where a password goes: in the datasource URL.
        let line = text
            .lines()
            .find(|l| l.starts_with("url = "))
            .expect("the sample has a datasource.url")
            .to_owned();
        text.replace(
            &line,
            &format!("url = \"postgres://bbs:{CANARY}@db.example/bbs\""),
        )
    })
}

fn check(text: &str) {
    let input = FileInput {
        path: Path::new(FILE),
        text,
        loose_permissions: false,
    };
    match assemble(Some(input), &[], &[]) {
        Ok(got) => {
            // What loaded does not show the password, in a message, a warning or `Debug`.
            for warning in &got.warnings {
                assert!(!warning.contains(CANARY), "{warning}");
            }
            assert!(!format!("{:?}", got.config).contains(CANARY));
            // Board text that loaded holds nothing the sanitizer would change: no control
            // character, no marker codepoint, no Nerd Font glyph.
            let board = &got.config.board;
            let widgets = &got.config.widgets;
            for text in [
                &board.name,
                &board.tagline,
                &board.location,
                &board.since,
                &board.ticker,
                &board.quotes,
                &widgets.motd,
                &widgets.quotes,
            ] {
                assert!(
                    !text.chars().any(crate::fuzz::forbidden_in_content),
                    "{text:?}"
                );
            }
        }
        Err(error) => {
            let shown = error.to_string();
            assert!(!shown.contains(CANARY), "password in an error: {shown}");
            assert!(!format!("{error:?}").contains(CANARY), "password in Debug");
            if let Err(why) = reason_is_loggable(&shown.replace('\n', " ")) {
                panic!("{why}");
            }
        }
    }
}

#[test]
fn the_sample_is_a_valid_config_so_the_mutations_start_from_something_real() {
    let input = FileInput {
        path: Path::new(FILE),
        text: sample(),
        loose_permissions: false,
    };
    if let Err(error) = assemble(Some(input), &[], &[]) {
        panic!("sample refused: {error}\n{}", sample());
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn config_mutated(text in mutated(sample())) {
        check(&text);
    }

    #[test]
    fn config_garbage(text in garbage()) {
        check(&text);
    }

    #[test]
    fn config_random_bytes(text in random_text()) {
        check(&text);
    }
}

/// A config file over the cap is refused at the read, before it is held whole or parsed, and
/// the reason names the cap.
#[test]
fn an_oversized_config_file_is_refused_unread() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let line = format!("# {}\n", "x".repeat(100));
    std::fs::write(&path, line.repeat(super::MAX_CONFIG_BYTES / line.len() + 2)).unwrap();
    let result = Loaded::load(Some(path.clone()), &[], &[], Dirs::default());
    match result {
        Err(ConfigError::Read { path: p, cause }) => {
            assert_eq!(p, path);
            assert!(cause.to_string().contains("larger than"), "{cause}");
        }
        other => panic!("{other:?}"),
    }
    // Just under the cap still loads.
    std::fs::write(&path, "# only a comment\n").unwrap();
    assert!(Loaded::load(Some(path), &[], &[], Dirs::default()).is_ok());
}

/// Not UTF-8 is a read error, not a panic.
#[test]
fn a_config_file_that_is_not_text_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, [0xff, 0xfe, 0x00, 0x80]).unwrap();
    assert!(matches!(
        Loaded::load(Some(path), &[], &[], Dirs::default()),
        Err(ConfigError::Read { .. })
    ));
}

/// A refusal does not repeat a 50 000 character value or key whole.
#[test]
fn a_config_refusal_never_repeats_a_long_value_or_key_whole() {
    let long = "z".repeat(50_000);
    for text in [
        format!("[widgets]\nhidden = \"{long}\"\n"),
        format!("{long} = 1\n"),
        format!("[board]\n{long} = 1\n"),
        format!("[{long}]\nx = 1\n"),
        format!("[ssh]\nlisten = \"{long}\"\n"),
    ] {
        let input = FileInput {
            path: Path::new(FILE),
            text: &text,
            loose_permissions: false,
        };
        let shown = assemble(Some(input), &[], &[]).unwrap_err().to_string();
        assert!(
            shown.chars().count() < 450,
            "{} characters",
            shown.chars().count()
        );
        assert!(!shown.chars().any(char::is_control), "{shown:?}");
    }
}
