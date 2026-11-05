//! Carrying out the sysop's menus-and-pages requests (UI-6): list and reload.
//!
//! The Menus and pages screen (core) turns a key into a [`MenuToolsRequest`]; this does the work
//! and returns what to show and what to audit. It is synchronous and reads only `menus.toml` and
//! `pages/` in the board's data directory; the caller runs it off the async threads and writes
//! the audit row.
//!
//! **Permission.** Every request is checked against the caller's [`Snapshot`] for
//! `themes.manage` here, at the moment it runs, whatever the screen thought (D33). A refusal
//! changes nothing and audits nothing. **Audit.** A reload that changes what members get returns
//! a `menus.reloaded` event; a reload that changes nothing, or a refused request, returns none
//! (the log holds changes, not attempts). **A bad file** never replaces a good menu with a
//! broken one: the load falls back to the built-in menu and says why, exactly as at start.

use russh_bbs_core::access::{Permission, RoleInfo, Slug, Snapshot, Via};
use russh_bbs_core::audit::{AuditEvent, events};
use russh_bbs_core::menu::{MenuToolsListing, MenuToolsRequest, PageRow, RoleRow, outline};

use super::MenuSetup;
use crate::ui::Ui;

/// What a request produced.
#[derive(Debug)]
pub(crate) struct Outcome {
    /// The screen's new listing.
    pub(crate) listing: MenuToolsListing,
    /// One line saying what happened.
    pub(crate) message: String,
    /// The audit row to write, when something changed.
    pub(crate) event: Option<AuditEvent>,
}

/// Run `request` for the holder of `access`. `roles` are the board's roles, for the preview.
#[must_use]
pub(crate) fn run(
    ui: &Ui,
    access: &Snapshot,
    roles: &[RoleInfo],
    request: MenuToolsRequest,
) -> Outcome {
    let denied = || Outcome {
        listing: MenuToolsListing::default(),
        message: "You are not allowed to manage menus and pages.".to_owned(),
        event: None,
    };
    if access.require(Permission::ThemesManage, None).is_err() {
        return denied();
    }
    let Some(actor) = access.account() else {
        return denied();
    };
    if !ui.has_menu_dir() {
        return Outcome {
            listing: MenuToolsListing {
                unavailable: true,
                ..MenuToolsListing::default()
            },
            message: "This board was started without a data directory.".to_owned(),
            event: None,
        };
    }
    let mut message = String::new();
    let mut event = None;
    if request == MenuToolsRequest::Reload
        && let Some((setup, changed)) = ui.reload_menus()
    {
        let state = if setup.problem().is_some() {
            "refused"
        } else if setup.custom() {
            "menus.toml"
        } else {
            "built-in"
        };
        message = if let Some(reason) = setup.problem() {
            format!("Reloaded: menus.toml refused, the built-in menu is in use: {reason}.")
        } else if changed {
            "Reloaded. New sessions get it; people signed in keep their current menu.".to_owned()
        } else {
            "Reloaded: nothing changed.".to_owned()
        };
        if changed {
            event = Some(events::menus_reloaded(actor, state, setup.pages().count()));
        }
    }
    Outcome {
        listing: listing(&ui.menus(), access, roles),
        message,
        event,
    }
}

/// What the screen shows for the loaded setup: one effective menu per role, and the pages.
fn listing(setup: &MenuSetup, access: &Snapshot, roles: &[RoleInfo]) -> MenuToolsListing {
    let mut rows: Vec<RoleRow> = roles
        .iter()
        .map(|role| {
            let held = Snapshot::from_role(
                None,
                &role.name,
                role.rank,
                &Via::Account,
                role.permissions.iter().copied(),
            );
            RoleRow {
                name: role.name.as_str().to_owned(),
                lines: outline(setup.tree(), &held),
            }
        })
        .collect();
    // Lowest rank first, so the first role shown is the plainest member.
    rows.sort_by_key(|row| {
        roles
            .iter()
            .find(|role| role.name.as_str() == row.name)
            .map_or(0, |role| role.rank)
    });
    if rows.is_empty() {
        rows.push(RoleRow {
            name: Slug::SYSOP.as_str().to_owned(),
            lines: outline(setup.tree(), access),
        });
    }
    MenuToolsListing {
        custom: setup.custom(),
        problem: setup.problem().map(str::to_owned),
        roles: rows,
        pages: setup
            .pages()
            .map(|page| PageRow {
                id: page.id().to_owned(),
                title: page.title().to_owned(),
            })
            .collect(),
        unavailable: false,
    }
}
