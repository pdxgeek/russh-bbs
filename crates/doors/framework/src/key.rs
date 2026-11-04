//! The keys a door sees. The board's own key type stays in the server; a door gets this small
//! one, so a game cannot depend on how the terminal was parsed.

/// One keypress. Ctrl-C and Ctrl-D are not here: they log the member off from any screen (D54)
/// and are handled by the board before a door sees a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DoorKey {
    /// A printable character.
    Char(char),
    /// Enter.
    Enter,
    /// Escape. A door normally treats it as "back", and as "leave" at its top menu.
    Escape,
    /// Backspace.
    Backspace,
    /// Up arrow.
    Up,
    /// Down arrow.
    Down,
    /// Left arrow.
    Left,
    /// Right arrow.
    Right,
}

impl DoorKey {
    /// The key as one character of a replay log (see `replay`): `a`-`z`, digits and the like as
    /// themselves, the named keys as the control letters `\n`, `\u{1b}`, ... spelled out by
    /// [`DoorKey::token`]. Used by logs, so it must stay stable.
    #[must_use]
    pub fn token(self) -> String {
        match self {
            Self::Char(' ') => "space".to_owned(),
            Self::Char(c) if c.is_ascii_alphanumeric() => c.to_string(),
            Self::Char(c) => format!("u{:x}", u32::from(c)),
            Self::Enter => "enter".to_owned(),
            Self::Escape => "esc".to_owned(),
            Self::Backspace => "bs".to_owned(),
            Self::Up => "up".to_owned(),
            Self::Down => "down".to_owned(),
            Self::Left => "left".to_owned(),
            Self::Right => "right".to_owned(),
        }
    }

    /// The key a [`DoorKey::token`] names.
    #[must_use]
    pub fn from_token(token: &str) -> Option<Self> {
        match token {
            "space" => Some(Self::Char(' ')),
            "enter" => Some(Self::Enter),
            "esc" => Some(Self::Escape),
            "bs" => Some(Self::Backspace),
            "up" => Some(Self::Up),
            "down" => Some(Self::Down),
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            _ => {
                let mut chars = token.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) if c.is_ascii_alphanumeric() => Some(Self::Char(c)),
                    _ => token
                        .strip_prefix('u')
                        .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                        .and_then(char::from_u32)
                        .map(Self::Char),
                }
            }
        }
    }
}
