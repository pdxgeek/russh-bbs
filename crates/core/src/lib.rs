//! Business rules for russh-bbs: domain types, traits, the shared sanitizer and the icon
//! registry. Front ends and storage depend on this crate; it depends on none of them.

pub mod access;
pub mod audit;
pub mod chat;
pub mod credits;
pub mod drafts;
pub mod forums;
pub mod founder;
pub mod icons;
pub mod invites;
pub mod limits;
pub mod menu;
pub mod passwords;
pub mod profiles;
pub mod queue;
pub mod registration;
pub mod sanitize;
pub mod scheduler;
pub mod store;
pub mod terminal_settings;
pub mod waiting_room;

/// The crate version, used by front ends to report what they were built against.
#[must_use]
pub const fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::version;

    #[test]
    fn version_is_not_empty() {
        assert_ne!(version(), "");
    }
}
