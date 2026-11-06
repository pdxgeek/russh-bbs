//! Story arcs and endings (M4-L4): six arcs of linked scenes, each with branches that close
//! doors and several endings. The scenes are ordinary transit events (`content/arcs.toml`)
//! gated by the captain's place in the arc; this file holds the state a save keeps.

use crate::codec::{Reader, Writer};

/// How many arcs there are: one per faction and a mystery about why the gates failed.
pub const ARC_COUNT: usize = 6;
/// "No ending yet" in a save.
pub const NO_ENDING: u8 = 255;

/// Where a captain is in one arc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArcState {
    /// Steps finished (the next scene is step `step + 1`).
    pub step: u8,
    /// Branch bits set by choices.
    pub branches: u16,
    /// The real day the last step happened (steps are spaced out).
    pub day: i64,
    /// The ending reached, or [`NO_ENDING`].
    pub ending: u8,
}

impl Default for ArcState {
    fn default() -> Self {
        Self {
            step: 0,
            branches: 0,
            day: 0,
            ending: NO_ENDING,
        }
    }
}

impl ArcState {
    /// Whether the arc is over.
    #[must_use]
    pub const fn finished(&self) -> bool {
        self.ending != NO_ENDING
    }

    /// Write to a save.
    pub fn write(&self, w: &mut Writer) {
        w.u8(self.step);
        w.u16(self.branches);
        w.i64(self.day);
        w.u8(self.ending);
    }

    /// Read from a save.
    pub fn read(r: &mut Reader<'_>) -> Option<Self> {
        Some(Self {
            step: r.u8()?,
            branches: r.u16()?,
            day: r.i64()?,
            ending: r.u8()?,
        })
    }
}
