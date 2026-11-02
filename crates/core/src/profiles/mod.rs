//! Member profiles and the members list (M2-F).
//!
//! * [`fields`]: the five fixed fields, their caps, and the sanitizing of every one.
//! * [`store`]: the [`ProfileStore`] trait (both backends implement it) and the paging types.
//! * [`service`]: the actions, each taking a `Granted` from `core::access`, plus [`execute`],
//!   which runs a screen's [`Request`] after checking the permission.
//! * [`screen`]: the pure state machine of the list, profile and edit screens, which emits
//!   string-table keys and sanitized content for a front end to draw.
//!
//! **Honesty.** A profile is readable by every member (`members.view`) and by the sysop. The
//! edit screen says so; nothing here is called private and no field is locked. There are no
//! avatars and no custom fields, and the website is plain text that is never fetched or
//! linked.

mod fields;
pub mod keys;
mod screen;
mod service;
mod store;

pub use fields::{
    ABOUT_MAX, DISPLAY_NAME_MAX, Field, LOCATION_MAX, PRONOUNS_MAX, Profile, RawProfile,
    WEBSITE_MAX,
};
pub use keys::TEXT_KEYS;
pub use screen::{Line, MembersScreen, ScreenKey, ScreenView, Step};
pub use service::{
    MemberPage, ProfileError, Request, Response, Saved, edit_grant, edit_profile, execute,
    list_members, load_for_edit, view_profile,
};
pub use store::{
    Cursor, HandlePrefix, MemberQuery, MemberRecord, MemberSummary, PAGE_SIZE, ProfileStore,
};

#[cfg(test)]
mod tests;
