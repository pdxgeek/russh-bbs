//! The glyph test: "do these look like icons?", asked one tier at a time.
//!
//! The server cannot see the client's font (`docs/ui.md`, "Icons"), so a person tells it. The
//! test offers the richest tier first. A "yes" picks that tier; a "no" moves to the next; a
//! "no" to every glyph tier, or skipping, leaves the safe fallback (ASCII, no color) on the
//! first run. Re-running the test later and skipping changes nothing.

use super::settings::TerminalSettings;
use crate::icons::{ContentIcon, Icon, Tier};

/// Which tier is on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlyphStep {
    /// Private-use Nerd Font glyphs.
    NerdFont,
    /// Plain Unicode symbols.
    Unicode,
}

impl GlyphStep {
    /// The tier this step shows.
    #[must_use]
    pub const fn tier(self) -> Tier {
        match self {
            Self::NerdFont => Tier::NerdFont,
            Self::Unicode => Tier::Unicode,
        }
    }
}

/// A person's answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlyphAnswer {
    /// "These look like icons."
    Yes,
    /// "They do not."
    No,
    /// Escape: leave the question unanswered.
    Skip,
}

/// What an answer led to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlyphProgress {
    /// Show the next step.
    Next,
    /// The test is over; store these settings.
    Chosen(TerminalSettings),
    /// A re-run was skipped; change nothing.
    Cancelled,
}

/// One run of the glyph test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlyphTest {
    step: GlyphStep,
    first_run: bool,
}

impl GlyphTest {
    /// Start at the richest tier. `first_run` is true when no tier is stored yet, in which case
    /// skipping stores the safe fallback; on a re-run skipping cancels.
    #[must_use]
    pub const fn new(first_run: bool) -> Self {
        Self {
            step: GlyphStep::NerdFont,
            first_run,
        }
    }

    /// The step on screen.
    #[must_use]
    pub const fn step(&self) -> GlyphStep {
        self.step
    }

    /// Whether this is the first-login run.
    #[must_use]
    pub const fn is_first_run(&self) -> bool {
        self.first_run
    }

    /// Apply an answer to `current` settings.
    pub fn answer(&mut self, answer: GlyphAnswer, current: TerminalSettings) -> GlyphProgress {
        match (answer, self.step) {
            (GlyphAnswer::Yes, step) => GlyphProgress::Chosen(current.with_tier(step.tier())),
            (GlyphAnswer::No, GlyphStep::NerdFont) => {
                self.step = GlyphStep::Unicode;
                GlyphProgress::Next
            }
            (GlyphAnswer::No, GlyphStep::Unicode) => GlyphProgress::Chosen(current.safe_fallback()),
            (GlyphAnswer::Skip, _) if self.first_run => {
                GlyphProgress::Chosen(current.safe_fallback())
            }
            (GlyphAnswer::Skip, _) => GlyphProgress::Cancelled,
        }
    }

    /// The sample glyphs for the current step, drawn from the registry, never from raw
    /// codepoints. Content icons only: the test shows no security marker.
    #[must_use]
    pub fn samples(&self) -> String {
        sample_row(self.step.tier())
    }
}

/// The content icons for `tier`, separated by two spaces. Each is padded to its table width.
fn sample_row(tier: Tier) -> String {
    ContentIcon::ALL
        .iter()
        .map(|icon| Icon::from(*icon).render(tier).plain().to_owned())
        .collect::<Vec<_>>()
        .join("  ")
}
