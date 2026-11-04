//! How a new account is tied to the person who made it (D94): by the SSH key they presented, or
//! by a password they chose. One shared shape for the three transactions that create an account
//! from the waiting room (redeem, `open`, approve), so each backend binds in one place.

/// What the new account is bound to. Borrowed from the caller; nothing here is logged.
#[derive(Clone, Copy)]
pub(crate) enum Binding<'a> {
    /// An SSH key's public fingerprint: one `account_keys` row.
    Key(&'a str),
    /// An argon2id PHC hash of a password the person chose: one `account_password` row and no
    /// key. The account can sign in with the password only (over telnet, and over ssh when the
    /// sysop has opted in).
    Password(&'a str),
    /// Both: a visitor with an SSH key who also chose a password (D94 (6)). The key is the
    /// everyday login, the password the way back in.
    KeyAndPassword(&'a str, &'a str),
}
