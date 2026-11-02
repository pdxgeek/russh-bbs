//! The daily cap on accounts made by `open` registration: a stored sysop setting next to
//! `registration_mode` (`open_daily_cap`, D73).

/// The cap when the sysop has not set one: 25 new accounts per UTC day. A small board gets a
/// handful of strangers a day; 25 lets a good day through and stops a script cold.
pub const DEFAULT_OPEN_DAILY_CAP: u32 = 25;

/// The largest cap that is honoured. A larger stored value reads as the default, so a typo
/// cannot quietly turn the cap off.
pub const MAX_OPEN_DAILY_CAP: u32 = 100_000;

/// How many accounts `open` registration may create in one UTC day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DailyCap {
    /// At most this many (1 to [`MAX_OPEN_DAILY_CAP`]).
    Limited(u32),
    /// No cap: the sysop stored `0`. The only way to turn the cap off.
    Unlimited,
}

impl DailyCap {
    /// Parse a stored value: a number from 1 to [`MAX_OPEN_DAILY_CAP`], or `0` for no cap.
    /// `None` for anything else.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().parse::<u32>().ok()? {
            0 => Some(Self::Unlimited),
            n if n <= MAX_OPEN_DAILY_CAP => Some(Self::Limited(n)),
            _ => None,
        }
    }

    /// Read the stored setting. Unset, blank, not a number or out of range is the default
    /// ([`DEFAULT_OPEN_DAILY_CAP`]), never "no cap"; `0` is the deliberate way to say no cap.
    #[must_use]
    pub fn from_setting(stored: Option<&str>) -> Self {
        stored
            .and_then(Self::parse)
            .unwrap_or(Self::Limited(DEFAULT_OPEN_DAILY_CAP))
    }

    /// The number of accounts, if there is a cap.
    #[must_use]
    pub const fn limit(self) -> Option<u32> {
        match self {
            Self::Limited(n) => Some(n),
            Self::Unlimited => None,
        }
    }
}
