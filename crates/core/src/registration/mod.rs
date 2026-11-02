//! Registration modes `apply` and `open` (M2-G, D2, D23, D73, D74).
//!
//! * **`apply`:** the waiting room's application form hands a [`InviteAsk`](crate::queue::InviteAsk)
//!   of kind [`RequestKind::Application`](crate::queue::RequestKind) to [`submit_application`],
//!   which checks `membership.apply` and the stored mode, then uses M2-E's queue as it is: its
//!   table, its one-per-key rule, its cap, its rate limits, its expiry, and the sysop's
//!   approve/deny screen. There is no second queue. The applicant's key has no account, so it
//!   keeps reaching the waiting room until a sysop approves (which makes an `active` account).
//! * **`open`:** the create-account form calls [`open_account`], which checks
//!   `membership.open` and the stored mode, then the per-key and per-network gate, then makes an
//!   `active` account in one store transaction (`invited_by` NULL; nobody vouched). The same
//!   transaction counts the account against the day's cap ([`DailyCap`]), so two sessions cannot
//!   take the last place. Every refusal the person can see is one [`OpenError`] and one
//!   sentence on screen; the reason is logged.
//! * **The cap degrades `open` to `apply`** for the rest of the UTC day: the room is built in
//!   `apply` mode with an honest notice ([`RegistrationStore::open_cap_reached`]), and an
//!   attempt that meets the cap is refused. The first refusal of a day writes one
//!   `registration.capped` audit row (never more, so strangers cannot grow the log).
//!
//! **No oracle.** Neither form's answer depends on who is on the board or how full the queue
//! is. The one thing a stranger can read about the cap is the degrade notice itself, which D2
//! asks for.

mod cap;
mod service;
mod store;

pub use cap::{DEFAULT_OPEN_DAILY_CAP, DailyCap, MAX_OPEN_DAILY_CAP};
pub use service::{
    DEFAULT_OPEN_KEY_RATE, DEFAULT_OPEN_NETWORK_RATE, OpenGate, daily_cap, open_account,
    open_cap_reached, submit_application,
};
pub use store::{OpenError, OpenRefusal, Opened, RegistrationStore};

#[cfg(test)]
mod tests;
