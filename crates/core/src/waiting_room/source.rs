//! Where the waiting room gets its settings: the registration mode and the sysop's banner and
//! rules text.
//!
//! The server asks a [`WaitingRoomSource`] once per session. Any store that implements
//! [`SettingsStore`] is one. A source that fails makes the room fall back to the strictest
//! built-in configuration ([`RoomConfig::default`]: `invite` mode, default text), never to a
//! looser mode.

use std::future::Future;

use crate::access::NoMembers;
use crate::store::{RegistrationMode, SettingKey, SettingsStore, StoreError, registration_mode};

/// What the waiting room needs from the board's settings.
///
/// Banner and rules are raw sysop text. They are sanitized when the room is built, not here.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RoomConfig {
    /// Which screens an unknown key sees.
    pub mode: RegistrationMode,
    /// The sysop's banner, if set.
    pub banner: Option<String>,
    /// The sysop's rules, if set.
    pub rules: Option<String>,
}

/// Loads the waiting room's configuration.
pub trait WaitingRoomSource: Send + Sync {
    /// The current settings. On error the caller uses [`RoomConfig::default`].
    fn load_room_config(&self) -> impl Future<Output = Result<RoomConfig, StoreError>> + Send;
}

impl<S: SettingsStore> WaitingRoomSource for S {
    async fn load_room_config(&self) -> Result<RoomConfig, StoreError> {
        Ok(RoomConfig {
            mode: registration_mode(self).await?,
            banner: non_blank(self.get_setting(SettingKey::WaitingRoomBanner).await?),
            rules: non_blank(self.get_setting(SettingKey::WaitingRoomRules).await?),
        })
    }
}

/// The empty directory has no settings either: defaults.
impl WaitingRoomSource for NoMembers {
    fn load_room_config(&self) -> impl Future<Output = Result<RoomConfig, StoreError>> + Send {
        std::future::ready(Ok(RoomConfig::default()))
    }
}

/// A stored blank string means "not set".
fn non_blank(value: Option<String>) -> Option<String> {
    value.filter(|text| !text.trim().is_empty())
}
