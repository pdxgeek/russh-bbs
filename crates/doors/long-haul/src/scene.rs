//! What a captain is in the middle of: an event waiting for a choice, a fight, or a boarding.
//!
//! A scene is saved with the captain. While one is open the only actions allowed are its own
//! (`Choose`, `Fight`, `Run`, `Wreck`, `Skirmish`), so dropping the line mid-scene resumes it
//! and cannot be used to reroll it.

use crate::boarding::Boarding;
use crate::codec::{Reader, Writer};
use crate::combat::Combat;
use crate::content::Content;
use crate::frontier::Frontier;

/// An open scene.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scene {
    /// A transit event waiting for the captain's choice (an index into the content's events).
    Event(u16),
    /// A ship fight.
    Combat(Box<Combat>),
    /// A boarding party at work.
    Boarding(Box<Boarding>),
}

impl Scene {
    /// Write to a save (a tag byte, then the body).
    pub fn write(&self, w: &mut Writer) {
        match self {
            Self::Event(e) => {
                w.u8(1);
                w.u16(*e);
            }
            Self::Combat(c) => {
                w.u8(2);
                c.write(w);
            }
            Self::Boarding(b) => {
                w.u8(3);
                b.write(w);
            }
        }
    }

    /// Read an optional scene: a leading `0` means none.
    pub fn read(
        r: &mut Reader<'_>,
        content: &Content,
        crew_len: usize,
        _frontier: &Frontier,
    ) -> Option<Option<Self>> {
        Some(match r.u8()? {
            0 => None,
            1 => {
                let e = r.u16()?;
                if usize::from(e) >= content.transit.event.len() {
                    return None;
                }
                Some(Self::Event(e))
            }
            2 => Some(Self::Combat(Box::new(Combat::read(r, content)?))),
            3 => Some(Self::Boarding(Box::new(Boarding::read(
                r, content, crew_len,
            )?))),
            _ => return None,
        })
    }
}
