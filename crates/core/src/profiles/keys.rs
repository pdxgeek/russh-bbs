//! The string-table keys the members screens use. Core emits keys and sanitized content, never
//! fixed text, so a theme or translation can change every word (`docs/ui.md`). The server's
//! string table must hold exactly these keys plus the menu tree's ([`TEXT_KEYS`]).

macro_rules! keys {
    ($($name:ident = $key:literal),+ $(,)?) => {
        $(
            #[doc = concat!("String-table key `", $key, "`.")]
            pub const $name: &str = $key;
        )+

        /// Every key above.
        pub const TEXT_KEYS: &[&str] = &[$($name),+];
    };
}

keys! {
    LIST_TITLE = "members.list.title",
    LIST_SEARCH = "members.list.search",
    LIST_SEARCH_NONE = "members.list.search_none",
    LIST_EMPTY = "members.list.empty",
    LIST_NO_MATCH = "members.list.no_match",
    LIST_PAGE = "members.list.page",
    LIST_FOOTER = "members.list.footer",
    LIST_FOOTER_SEARCH = "members.list.footer_search",
    VIEW_TITLE = "members.view.title",
    VIEW_FOOTER = "members.view.footer",
    VIEW_FOOTER_EDIT = "members.view.footer_edit",
    EDIT_TITLE = "members.edit.title",
    EDIT_TITLE_OTHER = "members.edit.title_other",
    EDIT_READABLE = "members.edit.readable",
    EDIT_PLAIN_TEXT = "members.edit.plain_text",
    EDIT_FOOTER = "members.edit.footer",
    LABEL_DISPLAY_NAME = "members.label.display_name",
    LABEL_PRONOUNS = "members.label.pronouns",
    LABEL_LOCATION = "members.label.location",
    LABEL_ABOUT = "members.label.about",
    LABEL_WEBSITE = "members.label.website",
    LABEL_HANDLE = "members.label.handle",
    VALUE_NOT_SET = "members.value.not_set",
    NOTICE_SAVED = "members.notice.saved",
    NOTICE_UNCHANGED = "members.notice.unchanged",
    NOTICE_NOT_FOUND = "members.notice.not_found",
    NOTICE_FAILED = "members.notice.failed",
    NOTICE_RANK = "members.notice.rank",
}
