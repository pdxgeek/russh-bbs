//! The vocabulary of the community features and every bound, written once.

use std::fmt;

use crate::access::Permission;
use crate::drafts::{Body, BodyError, BodyLimits};
use crate::sanitize::SingleLine;

/// Members shown in last callers.
pub const CALLERS_SHOWN: usize = 15;
/// Polls on one page of the poll list. Nine so `1`-`9` pick a row.
pub const POLL_PAGE: usize = 9;
/// Characters in a poll's question.
pub const QUESTION_MAX: usize = 60;
/// Characters in one answer option.
pub const OPTION_MAX: usize = 40;
/// Fewest options a poll has.
pub const OPTIONS_MIN: usize = 2;
/// Most options a poll has (`1`-`6` vote).
pub const OPTIONS_MAX: usize = 6;
/// Characters in a poll's optional description.
pub const DESCRIPTION_CHARS: usize = 350;
/// Lines in a poll's optional description.
pub const DESCRIPTION_LINES: usize = 5;

macro_rules! id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(i64);

        impl $name {
            /// Wrap a raw id. Only the store, which maps rows, should call this.
            #[must_use]
            pub const fn from_raw(raw: i64) -> Self {
                Self(raw)
            }

            /// The raw id, for the store to bind into a query.
            #[must_use]
            pub const fn as_raw(self) -> i64 {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

id! {
    /// Store-assigned id of a one-liner or a wall line.
    EntryId
}
id! {
    /// Store-assigned id of a poll.
    PollId
}

/// The two lists of short lines. They share one table and one set of rules; only the numbers
/// differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LineKind {
    /// A one-liner: one short line in a list, newest first.
    Oneliner,
    /// A line on the graffiti wall, a smaller list that rolls off faster.
    Graffiti,
}

impl LineKind {
    /// Both kinds.
    pub const ALL: [Self; 2] = [Self::Oneliner, Self::Graffiti];

    /// The stored name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Oneliner => "oneliner",
            Self::Graffiti => "graffiti",
        }
    }

    /// Parse a stored name.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.name() == text)
    }

    /// Characters in one line.
    #[must_use]
    pub const fn max_chars(self) -> usize {
        match self {
            Self::Oneliner => 70,
            Self::Graffiti => 60,
        }
    }

    /// Lines kept, and shown. Adding past it deletes the oldest in the same transaction, so
    /// the list is bounded and nothing is kept that cannot be seen.
    #[must_use]
    pub const fn keep(self) -> usize {
        match self {
            Self::Oneliner => 15,
            Self::Graffiti => 12,
        }
    }

    /// Lines one member may add in any rolling hour.
    #[must_use]
    pub const fn per_hour(self) -> usize {
        match self {
            Self::Oneliner => 5,
            Self::Graffiti => 10,
        }
    }

    /// The permission to add a line.
    #[must_use]
    pub const fn post_permission(self) -> Permission {
        match self {
            Self::Oneliner => Permission::OnelinersPost,
            Self::Graffiti => Permission::GraffitiPost,
        }
    }
}

/// Why typed text was refused as a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TextError {
    /// Nothing but whitespace (or nothing the sanitizer keeps).
    #[error("the line is empty")]
    Blank,
    /// Over the length of the list.
    #[error("the line is longer than {max} characters")]
    TooLong {
        /// The limit.
        max: usize,
    },
}

/// One line of text that passed the shared single-line sanitizer, is not blank and fits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineText(String);

impl LineText {
    /// Sanitize and check typed text. Over-long text is refused, not cut.
    ///
    /// # Errors
    /// [`TextError::Blank`] or [`TextError::TooLong`].
    pub fn parse(kind: LineKind, raw: &str) -> Result<Self, TextError> {
        parse_single(raw, kind.max_chars()).map(Self)
    }

    /// The text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Sanitize `raw` to one trimmed line of at most `max` characters, refusing blank or longer.
fn parse_single(raw: &str, max: usize) -> Result<String, TextError> {
    let clean = SingleLine::new(max.saturating_add(1)).sanitize(raw);
    let text = clean.as_str().trim();
    if text.is_empty() {
        return Err(TextError::Blank);
    }
    if text.chars().count() > max {
        return Err(TextError::TooLong { max });
    }
    Ok(text.to_owned())
}

/// Who sees a poll's counts, and when. Stored per poll.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResultsVisibility {
    /// Anyone who may read polls sees the counts at any time.
    Always,
    /// Counts show to a member who has voted, and to everyone once the poll is closed.
    #[default]
    AfterVote,
    /// Counts show only when the poll is closed.
    AfterClose,
}

impl ResultsVisibility {
    /// All three, in the order the form offers them.
    pub const ALL: [Self; 3] = [Self::Always, Self::AfterVote, Self::AfterClose];

    /// The stored name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Always => "always",
            Self::AfterVote => "after_vote",
            Self::AfterClose => "after_close",
        }
    }

    /// Parse a stored name.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.name() == text)
    }

    /// Whether counts are shown to a viewer who has (or has not) voted, on an open or closed
    /// poll.
    #[must_use]
    pub const fn shows(self, has_voted: bool, closed: bool) -> bool {
        closed
            || match self {
                Self::Always => true,
                Self::AfterVote => has_voted,
                Self::AfterClose => false,
            }
    }
}

/// Why typed poll text was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PollError {
    /// The question is blank.
    #[error("the question is empty")]
    QuestionBlank,
    /// The question is over [`QUESTION_MAX`].
    #[error("the question is longer than {QUESTION_MAX} characters")]
    QuestionTooLong,
    /// Fewer than [`OPTIONS_MIN`] non-blank options.
    #[error("a poll needs at least {OPTIONS_MIN} options")]
    TooFewOptions,
    /// More than [`OPTIONS_MAX`] options.
    #[error("a poll has at most {OPTIONS_MAX} options")]
    TooManyOptions,
    /// An option is over [`OPTION_MAX`].
    #[error("an option is longer than {OPTION_MAX} characters")]
    OptionTooLong,
    /// Two options read the same ignoring case.
    #[error("two options are the same")]
    DuplicateOption,
    /// The description is over its size.
    #[error(transparent)]
    Description(#[from] BodyError),
}

/// A poll as it may be stored: every text went through the shared sanitizer and fits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewPoll {
    question: String,
    options: Vec<String>,
    description: Option<Body>,
    visibility: ResultsVisibility,
}

impl NewPoll {
    /// The bounds of a poll's description.
    #[must_use]
    pub fn description_limits() -> BodyLimits {
        BodyLimits::new(DESCRIPTION_CHARS, DESCRIPTION_LINES)
    }

    /// Build a poll from typed text. Blank options are dropped (a form with an unused row is
    /// normal); a blank description is none.
    ///
    /// # Errors
    /// [`PollError`] for any text that is blank, too long, too many, too few or repeated.
    pub fn build(
        question: &str,
        options: &[String],
        description: &str,
        visibility: ResultsVisibility,
    ) -> Result<Self, PollError> {
        let question = parse_single(question, QUESTION_MAX).map_err(|e| match e {
            TextError::Blank => PollError::QuestionBlank,
            TextError::TooLong { .. } => PollError::QuestionTooLong,
        })?;
        let mut kept: Vec<String> = Vec::new();
        for raw in options {
            match parse_single(raw, OPTION_MAX) {
                Ok(text) => kept.push(text),
                Err(TextError::Blank) => {}
                Err(TextError::TooLong { .. }) => return Err(PollError::OptionTooLong),
            }
        }
        if kept.len() < OPTIONS_MIN {
            return Err(PollError::TooFewOptions);
        }
        if kept.len() > OPTIONS_MAX {
            return Err(PollError::TooManyOptions);
        }
        let mut seen: Vec<String> = Vec::new();
        for text in &kept {
            let lower = text.to_lowercase();
            if seen.contains(&lower) {
                return Err(PollError::DuplicateOption);
            }
            seen.push(lower);
        }
        let description = Body::prepare(Self::description_limits(), description)?;
        Ok(Self {
            question,
            options: kept,
            description: (!description.is_blank()).then_some(description),
            visibility,
        })
    }

    /// The question.
    #[must_use]
    pub fn question(&self) -> &str {
        &self.question
    }

    /// The options, in the order voted on (`1`-`6`).
    #[must_use]
    pub fn options(&self) -> &[String] {
        &self.options
    }

    /// The description, if any.
    #[must_use]
    pub fn description(&self) -> Option<&Body> {
        self.description.as_ref()
    }

    /// When counts show.
    #[must_use]
    pub const fn visibility(&self) -> ResultsVisibility {
        self.visibility
    }
}
