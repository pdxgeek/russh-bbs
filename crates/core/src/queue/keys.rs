//! The string-table keys the queue screens use. Core emits keys and sanitized content, never
//! fixed text, so a theme or translation can change every word (`docs/ui.md`). The server's
//! string table must hold exactly these keys plus the menu tree's and the other screens'
//! ([`TEXT_KEYS`]).

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
    TITLE_SYSOP = "queue.title.sysop",
    TITLE_VOUCH = "queue.title.vouch",
    INTRO_SYSOP = "queue.intro.sysop",
    INTRO_VOUCH = "queue.intro.vouch",
    EMPTY = "queue.empty",
    UNREAD = "queue.unread",
    ROW_META = "queue.row.meta",
    ROW_META_MINE = "queue.row.meta_mine",
    PAGE = "queue.page",
    FOOTER_LIST = "queue.footer.list",
    DETAIL_TITLE = "queue.detail.title",
    DETAIL_WHEN = "queue.detail.when",
    DETAIL_VOUCHES = "queue.detail.vouches",
    DETAIL_NO_VOUCHES = "queue.detail.no_vouches",
    DETAIL_MESSAGE = "queue.detail.message",
    ACTION_APPROVE = "queue.action.approve",
    ACTION_DENY = "queue.action.deny",
    ACTION_VOUCH = "queue.action.vouch",
    FOOTER_DETAIL = "queue.footer.detail",
    CONFIRM_APPROVE = "queue.confirm.approve",
    CONFIRM_DENY = "queue.confirm.deny",
    CONFIRM_VOUCH = "queue.confirm.vouch",
    FOOTER_CONFIRM = "queue.footer.confirm",
    NOTICE_APPROVED = "queue.notice.approved",
    NOTICE_DENIED = "queue.notice.denied",
    NOTICE_VOUCHED = "queue.notice.vouched",
    NOTICE_GONE = "queue.notice.gone",
    NOTICE_HANDLE_TAKEN = "queue.notice.handle_taken",
    NOTICE_KEY_HAS_ACCOUNT = "queue.notice.key_has_account",
    NOTICE_ALREADY_VOUCHED = "queue.notice.already_vouched",
    NOTICE_FAILED = "queue.notice.failed",
}
