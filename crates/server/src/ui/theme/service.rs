//! Carrying out the sysop's theme-pack requests (UI-7): list, export, import.
//!
//! The Theme packs screen (core) turns a key into a [`PackRequest`]; this module does the work
//! with the board's files and returns what to show and what to audit. It is synchronous and
//! touches only the three directories in [`PackDirs`](crate::ui::PackDirs); the caller runs it
//! off the async threads and writes the audit row.
//!
//! **Permission.** Every request is checked against the caller's [`Snapshot`] for
//! `themes.manage` here, at the moment it runs, whatever the screen thought (D33). A refusal
//! changes nothing and audits nothing. **Audit.** An export or an import that succeeds returns
//! its event; a refused or failed one returns none, so the log holds changes, not attempts.
//! **Import is validation first**: the pack file goes through [`pack::parse`], the same loaders
//! as a theme file and art on disk, before one byte is written, and it is installed with
//! `create_new` (nothing is overwritten). The style is loaded when the server next starts.
//! **Messages** are the screen's own plain words (chrome, not themeable); a reason that quotes a
//! file is sanitized again by the screen before it is shown.

use russh_bbs_core::access::{Permission, Snapshot};
use russh_bbs_core::audit::{AuditEvent, events};
use russh_bbs_core::terminal_settings::{FileRow, PackListing, PackRequest, StyleRow};

use super::pack;
use crate::ui::Ui;

/// What a request produced.
#[derive(Debug)]
pub struct Outcome {
    /// The screen's new listing.
    pub listing: PackListing,
    /// One line saying what happened.
    pub message: String,
    /// The audit row to write, when something changed.
    pub event: Option<AuditEvent>,
}

/// Run `request` for the holder of `access`.
#[must_use]
pub fn run(ui: &Ui, access: &Snapshot, request: PackRequest) -> Outcome {
    let denied = || Outcome {
        listing: PackListing::default(),
        message: "You are not allowed to manage themes.".to_owned(),
        event: None,
    };
    if access.require(Permission::ThemesManage, None).is_err() {
        return denied();
    }
    let Some(dirs) = ui.pack_dirs() else {
        return Outcome {
            listing: PackListing {
                unavailable: true,
                ..PackListing::default()
            },
            message: "This board was started without a packs directory.".to_owned(),
            event: None,
        };
    };
    let Some(actor) = access.account() else {
        return denied();
    };
    let base = ui.strings();
    let (message, event) = match request {
        PackRequest::List => (String::new(), None),
        PackRequest::Export(name) => {
            let result = pack::export(ui.theme_set(), ui.art(), name)
                .and_then(|text| pack::write_export(&dirs.packs, name, &text));
            match result {
                Ok(_) => (
                    format!("Exported {name} to packs/{name}.pack.toml."),
                    Some(events::theme_exported(actor, name.as_str())),
                ),
                Err(error) => (format!("Not exported: {error}."), None),
            }
        }
        PackRequest::Import(file) => {
            let result = file_in(&file)
                .and_then(|name| pack::read_file(&dirs.packs.join(name)))
                .and_then(|text| pack::parse(&text, base))
                .and_then(|parsed| {
                    pack::install(&parsed, ui.theme_set(), &dirs.themes, &dirs.art)
                        .map(|_| (parsed.theme().name(), parsed.art_files()))
                });
            match result {
                Ok((name, art)) => (
                    format!(
                        "Imported {name} ({art} art files). It loads when the server restarts."
                    ),
                    Some(events::theme_imported(actor, name.as_str(), art)),
                ),
                Err(error) => (format!("Not imported: {error}."), None),
            }
        }
    };
    Outcome {
        listing: listing(ui, &dirs.packs),
        message,
        event,
    }
}

/// A bare file name from the listing: no directory part, so a request cannot reach outside
/// `packs/` whatever the screen sent.
fn file_in(file: &str) -> Result<&str, pack::PackError> {
    let plain = !file.is_empty()
        && !file.contains(['/', '\\', '\0'])
        && file != "."
        && file != ".."
        && file.ends_with(".pack.toml");
    if plain {
        Ok(file)
    } else {
        Err(pack::PackError::Syntax(
            "that is not a pack file name".to_owned(),
        ))
    }
}

fn listing(ui: &Ui, packs: &std::path::Path) -> PackListing {
    let themes = ui.theme_set();
    PackListing {
        styles: themes
            .names()
            .into_iter()
            .map(|name| {
                let theme = themes.get(name);
                StyleRow {
                    name,
                    title: theme.title().to_owned(),
                    exportable: theme.export_text().is_some(),
                }
            })
            .collect(),
        files: pack::list_dir(packs, ui.strings())
            .into_iter()
            .map(|f| FileRow {
                file: f.file,
                verdict: f.verdict,
            })
            .collect(),
        unavailable: false,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use std::fs;

    use russh_bbs_core::access::{Slug, Snapshot, Via};
    use russh_bbs_core::audit::Action;
    use russh_bbs_core::store::AccountId;
    use russh_bbs_core::terminal_settings::ThemeName;

    use super::*;
    use crate::ui::PackDirs;

    fn snapshot(with_permission: bool) -> Snapshot {
        Snapshot::from_role(
            Some(AccountId::from_raw(7)),
            &Slug::SYSOP,
            100,
            &Via::Account,
            Permission::ALL
                .iter()
                .copied()
                .filter(|p| with_permission || *p != Permission::ThemesManage),
        )
    }

    fn board(root: &std::path::Path) -> Ui {
        Ui::standard_with_themes(&root.join("themes"))
            .unwrap()
            .with_art_dir(&root.join("art"))
            .with_pack_dirs(PackDirs {
                packs: root.join("packs"),
                themes: root.join("themes"),
                art: root.join("art"),
            })
    }

    fn name(text: &str) -> ThemeName {
        ThemeName::new(text).unwrap()
    }

    #[test]
    fn without_themes_manage_nothing_is_written_and_nothing_is_audited() {
        let dir = tempfile::tempdir().unwrap();
        let ui = board(dir.path());
        for request in [
            PackRequest::List,
            PackRequest::Export(name("neon")),
            PackRequest::Import("x.pack.toml".into()),
        ] {
            let outcome = run(&ui, &snapshot(false), request);
            assert!(outcome.event.is_none());
            assert!(outcome.message.contains("not allowed"));
            assert!(outcome.listing.styles.is_empty(), "a denial lists nothing");
        }
        assert!(!dir.path().join("packs").exists());
    }

    #[test]
    fn export_and_import_write_files_and_return_their_audit_events() {
        let dir = tempfile::tempdir().unwrap();
        let ui = board(dir.path());
        let sysop = snapshot(true);

        let listed = run(&ui, &sysop, PackRequest::List);
        assert!(listed.event.is_none());
        assert_eq!(listed.listing.styles.len(), 9);
        assert!(!listed.listing.styles[0].exportable);

        let exported = run(&ui, &sysop, PackRequest::Export(name("neon")));
        let event = exported.event.expect("an export is audited");
        assert_eq!(event.action, Action::ThemeExported);
        let file = dir.path().join("packs/neon.pack.toml");
        assert!(file.is_file());
        // A second export would replace a file: refused, and not audited.
        let again = run(&ui, &sysop, PackRequest::Export(name("neon")));
        assert!(again.event.is_none() && again.message.starts_with("Not exported"));

        let edited = fs::read_to_string(&file)
            .unwrap()
            .replace("name = \"neon\"", "name = \"neon3\"")
            .replace("style = \"neon\"", "style = \"neon3\"");
        fs::write(dir.path().join("packs/neon3.pack.toml"), edited).unwrap();
        let imported = run(&ui, &sysop, PackRequest::Import("neon3.pack.toml".into()));
        let event = imported.event.expect("an import is audited");
        assert_eq!(event.action, Action::ThemeImported);
        assert!(dir.path().join("themes/neon3.toml").is_file());
        // The next boot loads it, with its art.
        let next = board(dir.path());
        assert!(next.theme_set().contains(name("neon3")));
        assert!(
            next.art()
                .pieces()
                .iter()
                .any(|p| p.style() == Some(name("neon3")))
        );
    }

    #[test]
    fn a_refused_import_changes_nothing_and_is_not_audited() {
        let dir = tempfile::tempdir().unwrap();
        let ui = board(dir.path());
        let sysop = snapshot(true);
        fs::create_dir_all(dir.path().join("packs")).unwrap();
        fs::write(
            dir.path().join("packs/evil.pack.toml"),
            "pack = 1\ntheme = '''\nname = \"evil\"\n[style.marker]\nreverse = true\n'''\n",
        )
        .unwrap();
        let outcome = run(&ui, &sysop, PackRequest::Import("evil.pack.toml".into()));
        assert!(outcome.event.is_none());
        assert!(
            outcome.message.starts_with("Not imported"),
            "{}",
            outcome.message
        );
        assert!(!dir.path().join("themes").exists());
        // Names that leave the packs directory are refused before any file is opened.
        for bad in [
            "../x.pack.toml",
            "/etc/passwd",
            "a/b.pack.toml",
            "..",
            "x.toml",
            "",
        ] {
            let outcome = run(&ui, &sysop, PackRequest::Import(bad.into()));
            assert!(outcome.event.is_none(), "{bad}");
            assert!(outcome.message.starts_with("Not imported"), "{bad}");
        }
    }

    #[test]
    fn a_board_without_directories_says_so() {
        let ui = Ui::standard().unwrap();
        let outcome = run(&ui, &snapshot(true), PackRequest::List);
        assert!(outcome.listing.unavailable);
        assert!(outcome.event.is_none());
    }
}
