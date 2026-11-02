//! Paging long text, honoring the member's "pause" setting (FIX-M1-L8, D65).
//!
//! The rules live here and the front end only draws them. A [`Pager`] holds the lines of one
//! long text (already one screen row each: the front end wraps before it pages) and the
//! person's [`TerminalSettings::pause`]:
//!
//! * **Pause on:** the text is shown a page at a time with a "more" prompt. Every line is
//!   reachable; nothing is dropped.
//! * **Pause off:** no prompt and no paging. The first screenful is shown and the view says,
//!   in words, how many lines are not shown and that pause is how to read them. There is no
//!   scrollback on the alternate screen, so silently running off the end would hide text.
//!
//! Nothing here touches the terminal; `rows` is passed to every call because the window can
//! change between keys.

use super::TerminalSettings;

/// A key the pager understands. The front end maps raw keys to these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PagerKey {
    /// Space or Enter: the next page, or finish on the last one.
    Next,
    /// `b`: the previous page.
    Previous,
    /// Esc or `q`: stop reading.
    Stop,
}

/// What the front end does after a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PagerEffect {
    /// Redraw and keep going.
    Stay,
    /// Close the pager.
    Done,
}

/// What to say under the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PagerPrompt {
    /// The whole text is on screen. No prompt.
    None,
    /// Pause on, more pages follow.
    More {
        /// 1-based page being shown.
        page: usize,
        /// Pages in all.
        pages: usize,
    },
    /// Pause on, this is the last page of several.
    End {
        /// 1-based page being shown.
        page: usize,
        /// Pages in all.
        pages: usize,
    },
    /// Pause off and the text is longer than the screen: `hidden` lines are not shown.
    Clipped {
        /// Lines past the first screenful.
        hidden: usize,
    },
}

/// One page to draw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PagerView<'a> {
    /// The lines to show, at most `rows` of them.
    pub lines: &'a [String],
    /// The prompt for the row under them.
    pub prompt: PagerPrompt,
}

/// A long text and where the reader is in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pager {
    lines: Vec<String>,
    pause: bool,
    /// Index of the first line of the current page.
    top: usize,
}

impl Pager {
    /// Page `lines` according to `settings`.
    #[must_use]
    pub fn new(lines: Vec<String>, settings: &TerminalSettings) -> Self {
        Self {
            lines,
            pause: settings.pause(),
            top: 0,
        }
    }

    fn pages(&self, rows: usize) -> usize {
        self.lines.len().div_ceil(rows).max(1)
    }

    /// What to draw with `rows` rows available for text (at least one is assumed).
    #[must_use]
    pub fn view(&self, rows: usize) -> PagerView<'_> {
        let rows = rows.max(1);
        if !self.pause {
            let shown = rows.min(self.lines.len());
            let hidden = self.lines.len() - shown;
            return PagerView {
                lines: &self.lines[..shown],
                prompt: if hidden == 0 {
                    PagerPrompt::None
                } else {
                    PagerPrompt::Clipped { hidden }
                },
            };
        }
        let pages = self.pages(rows);
        // A resize can leave `top` mid-page; snap to the page that holds it.
        let page = self.top / rows;
        let start = page * rows;
        let end = (start + rows).min(self.lines.len());
        let prompt = if pages == 1 {
            PagerPrompt::None
        } else if page + 1 < pages {
            PagerPrompt::More {
                page: page + 1,
                pages,
            }
        } else {
            PagerPrompt::End {
                page: page + 1,
                pages,
            }
        };
        PagerView {
            lines: &self.lines[start..end],
            prompt,
        }
    }

    /// Handle one key with `rows` rows available for text.
    pub fn input(&mut self, key: PagerKey, rows: usize) -> PagerEffect {
        let rows = rows.max(1);
        match key {
            PagerKey::Stop => PagerEffect::Done,
            PagerKey::Previous => {
                self.top = (self.top / rows).saturating_sub(1) * rows;
                PagerEffect::Stay
            }
            PagerKey::Next => {
                let next = (self.top / rows + 1) * rows;
                if !self.pause || next >= self.lines.len() {
                    PagerEffect::Done
                } else {
                    self.top = next;
                    PagerEffect::Stay
                }
            }
        }
    }
}
