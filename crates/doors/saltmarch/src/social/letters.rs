//! Letters between clan leaders: templates, one optional line of the sender's own, and the
//! rules that keep them from becoming a way to grief.
//!
//! A letter is a template (a row of `content/letters.toml`) plus, if the sender adds one, a
//! single line that went through `Ctx::player_text` (sanitized, 140 characters, six a minute)
//! before it left the session. A template can require history: a clan that never won a raid
//! cannot gloat, one that never broke a pact cannot apologise for it. Limits are enforced in
//! the world edit: five letters a day, two unread per sender per recipient, a post that holds
//! only so much, and the recipient's block list (a blocked sender is told nothing).

use russh_bbs_door::content::{Table, plain_text};
use serde::Deserialize;

use super::{Body, ClanIx, Refusal, Wire, caps};
use crate::codec::{Reader, Writer};
use crate::world::World;

/// Letters table text.
pub const LETTERS_TOML: &str = include_str!("../../content/letters.toml");

/// The longest line a sender may add (the board's own cap).
pub const LINE_MAX: usize = russh_bbs_door::PLAYER_TEXT_MAX_CHARS;

/// What a letter is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// A greeting.
    Greeting,
    /// Thanks.
    Thanks,
    /// A warning.
    Warning,
    /// A taunt, which a raid can leave behind.
    Taunt,
    /// An apology.
    Apology,
    /// The words that go with a pact offer.
    Offer,
}

impl Kind {
    /// A word for the screen.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Greeting => "greeting",
            Self::Thanks => "thanks",
            Self::Warning => "warning",
            Self::Taunt => "taunt",
            Self::Apology => "apology",
            Self::Offer => "offer",
        }
    }
}

/// The history that opens a template.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Requires {
    /// Anyone.
    Always,
    /// The clan has won a raid.
    RaidsWon,
    /// The clan has been raided (or beaten a raid off).
    Raided,
    /// The clan has broken a pact.
    Betrayal,
    /// The clan has a pact in force.
    Pact,
    /// The clan has traded on the exchange.
    Trader,
}

impl Requires {
    /// What the player is told when the template is closed.
    #[must_use]
    pub const fn why(self) -> &'static str {
        match self {
            Self::Always => "",
            Self::RaidsWon => "needs a raid won",
            Self::Raided => "needs a raid on your hold",
            Self::Betrayal => "needs a pact broken",
            Self::Pact => "needs a pact in force",
            Self::Trader => "needs a trade on the exchange",
        }
    }
}

/// One template.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LetterDef {
    /// Its id (stored in the world, so never rename one).
    pub id: String,
    /// What kind.
    pub kind: Kind,
    /// What opens it.
    pub requires: Requires,
    /// The words, with `{clan}` and `{you}`.
    pub text: String,
}

impl LetterDef {
    /// The letter's words for a sender and a recipient.
    #[must_use]
    pub fn words(&self, from: &str, to: &str) -> String {
        self.text.replace("{clan}", from).replace("{you}", to)
    }
}

/// The table.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Letters {
    /// Every template.
    #[serde(rename = "letter")]
    pub entries: Vec<LetterDef>,
}

impl Table for Letters {
    fn validate(&self) -> Vec<String> {
        let mut p = Vec::new();
        let mut seen: Vec<&str> = Vec::new();
        for l in &self.entries {
            let ok_id = !l.id.is_empty()
                && l.id.len() <= 24
                && l.id.bytes().all(|b| b.is_ascii_lowercase() || b == b'-');
            if !ok_id {
                p.push(format!(
                    "[letter] id `{}` must be a-z and - (24 at most)",
                    l.id
                ));
            }
            if seen.contains(&l.id.as_str()) {
                p.push(format!("[letter] id `{}` is used twice", l.id));
            }
            seen.push(&l.id);
            if l.text.trim().is_empty()
                || l.text.chars().count() > 160
                || !plain_text(&l.text)
                || !l.text.contains("{clan}")
                || !l.text.contains("{you}")
            {
                p.push(format!(
                    "[letter] `{}`: plain text up to 160 with {{clan}} and {{you}}",
                    l.id
                ));
            }
        }
        for kind in [
            Kind::Greeting,
            Kind::Thanks,
            Kind::Warning,
            Kind::Taunt,
            Kind::Apology,
            Kind::Offer,
        ] {
            if !self.entries.iter().any(|l| l.kind == kind) {
                p.push(format!("[letter] none of kind {}", kind.name()));
            }
        }
        // A template anyone can send must exist for every kind a clan with no history can use.
        for kind in [Kind::Greeting, Kind::Warning, Kind::Offer] {
            if !self
                .entries
                .iter()
                .any(|l| l.kind == kind && l.requires == Requires::Always)
            {
                p.push(format!("[letter] no open template of kind {}", kind.name()));
            }
        }
        p
    }
}

/// A letter in the world, waiting for its recipient.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LetterBody {
    /// The sender's name.
    pub from: String,
    /// The template id.
    pub template: String,
    /// The sender's own line, already sanitized when it was sent.
    pub line: Option<String>,
    /// The day it was sent.
    pub day: i64,
}

impl Wire for LetterBody {
    fn write(&self, w: &mut Writer) {
        w.str(&self.from);
        w.str(&self.template);
        match &self.line {
            Some(l) => {
                w.bool(true);
                w.str(l);
            }
            None => w.bool(false),
        }
        w.i64(self.day);
    }

    fn read(r: &mut Reader<'_>) -> Option<Self> {
        Some(Self {
            from: r.str()?,
            template: r.str()?,
            line: if r.bool()? { Some(r.str()?) } else { None },
            day: r.i64()?,
        })
    }
}

impl World {
    /// Whether the history the world holds for `clan` opens a template.
    #[must_use]
    pub fn opens(&self, clan: ClanIx, requires: Requires) -> bool {
        let Some(c) = self.clans.get(usize::from(clan)) else {
            return false;
        };
        let r = &c.record;
        match requires {
            Requires::Always => true,
            Requires::RaidsWon => r.raids_won > 0,
            Requires::Raided => r.raided > 0 || r.repelled > 0,
            Requires::Betrayal => r.betrayals > 0,
            Requires::Pact => self.pacts_of(clan).next().is_some(),
            Requires::Trader => r.traded > 0,
        }
    }

    /// Send a letter. `line` has already been through the board's text rules; a line that
    /// would not survive them unchanged is dropped here as well (a second, cheap check).
    ///
    /// # Errors
    /// [`Refusal::Unknown`], [`Refusal::Terms`] (the template is closed to the sender),
    /// [`Refusal::LettersToday`], [`Refusal::Unread`], [`Refusal::Full`] or
    /// [`Refusal::MailFull`]. A recipient who blocked the sender gets nothing and the sender
    /// is told it was sent.
    pub fn send_letter(
        &mut self,
        letters: &Letters,
        from: ClanIx,
        to: ClanIx,
        template: &str,
        line: Option<&str>,
        day: i64,
    ) -> Result<(), Refusal> {
        let (Some(a), Some(b)) = (
            self.clans.get(usize::from(from)),
            self.clans.get(usize::from(to)),
        ) else {
            return Err(Refusal::Unknown);
        };
        if from == to {
            return Err(Refusal::Unknown);
        }
        let Some(def) = letters.entries.iter().find(|l| l.id == template) else {
            return Err(Refusal::Terms);
        };
        if !self.opens(from, def.requires) {
            return Err(Refusal::Terms);
        }
        // A taunt is for an enemy, not a partner.
        if def.kind == Kind::Taunt && self.pact_between(from, to).is_some() {
            return Err(Refusal::Pact);
        }
        let sent = if a.record.letters_day == day {
            a.record.letters_n
        } else {
            0
        };
        if sent >= caps::LETTERS_PER_DAY {
            return Err(Refusal::LettersToday);
        }
        let blocked = b.record.blocked.contains(&from);
        let waiting = self
            .social
            .mail
            .iter()
            .filter(|d| d.to == to && matches!(&d.body, Body::Letter(l) if l.from == a.name))
            .count();
        if !blocked {
            if waiting >= caps::UNREAD_PER_SENDER {
                return Err(Refusal::Unread);
            }
            let letters_total = self
                .social
                .mail
                .iter()
                .filter(|d| matches!(d.body, Body::Letter(_)))
                .count();
            if letters_total >= caps::LETTERS_TOTAL {
                return Err(Refusal::Full);
            }
            if !self.social.has_room(to) {
                return Err(Refusal::MailFull);
            }
        }
        let from_name = a.name.clone();
        if let Some(c) = self.clans.get_mut(usize::from(from)) {
            let r = &mut c.record;
            if r.letters_day != day {
                r.letters_day = day;
                r.letters_n = 0;
            }
            r.letters_n = r.letters_n.saturating_add(1);
        }
        if blocked {
            return Ok(());
        }
        let line = line
            .filter(|l| !l.trim().is_empty() && l.chars().count() <= LINE_MAX && plain_text(l))
            .map(str::to_owned);
        self.social.deliver(
            to,
            Body::Letter(LetterBody {
                from: from_name,
                template: template.to_owned(),
                line,
                day,
            }),
        );
        Ok(())
    }

    /// Stop `clan` receiving letters from `sender`.
    ///
    /// # Errors
    /// [`Refusal::Unknown`] or [`Refusal::Full`] (the block list is full).
    pub fn block(&mut self, clan: ClanIx, sender: ClanIx) -> Result<(), Refusal> {
        if usize::from(sender) >= self.clans.len() || clan == sender {
            return Err(Refusal::Unknown);
        }
        let Some(c) = self.clans.get_mut(usize::from(clan)) else {
            return Err(Refusal::Unknown);
        };
        if c.record.blocked.contains(&sender) {
            return Ok(());
        }
        if c.record.blocked.len() >= caps::BLOCK_CAP {
            return Err(Refusal::Full);
        }
        c.record.blocked.push(sender);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::assert_is_empty)]
    use super::*;
    use russh_bbs_door::content;

    fn table() -> Letters {
        content::load::<Letters>("letters", LETTERS_TOML).unwrap()
    }

    fn world() -> World {
        let mut w = World::new(1, 7, 100);
        for n in ["Ash", "Brindle", "Cove"] {
            w.register(n, 0);
        }
        w
    }

    #[test]
    fn the_table_loads_and_its_words_name_both_clans() {
        let t = table();
        assert!(t.entries.len() >= 12);
        let l = &t.entries[0];
        let words = l.words("Ash", "Brindle");
        assert!(words.contains("Ash") && words.contains("Brindle") && !words.contains('{'));
    }

    #[test]
    fn a_bad_table_is_named() {
        let bad = LETTERS_TOML.replacen("id = \"greeting-reeds\"", "id = \"Greeting Reeds\"", 1);
        assert!(content::load::<Letters>("letters", &bad).is_err());
        let dup = LETTERS_TOML.replacen("id = \"greeting-trade\"", "id = \"greeting-reeds\"", 1);
        assert!(content::load::<Letters>("letters", &dup).is_err());
        let ansi = LETTERS_TOML.replacen("The reeds are high", "The \u{1b}[31m reeds are high", 1);
        assert!(content::load::<Letters>("letters", &ansi).is_err());
    }

    #[test]
    fn history_opens_and_closes_templates() {
        let t = table();
        let mut w = world();
        assert_eq!(
            w.send_letter(&t, 0, 1, "taunt-keeps", None, 100),
            Err(Refusal::Terms),
            "no raid won, no gloating"
        );
        assert_eq!(
            w.send_letter(&t, 0, 1, "apology-oath", None, 100),
            Err(Refusal::Terms)
        );
        assert_eq!(
            w.send_letter(&t, 0, 1, "nonsense", None, 100),
            Err(Refusal::Terms)
        );
        w.clans[0].record.raids_won = 1;
        w.clans[0].record.betrayals = 1;
        assert_eq!(w.send_letter(&t, 0, 1, "taunt-keeps", None, 100), Ok(()));
        assert_eq!(w.send_letter(&t, 0, 2, "apology-oath", None, 100), Ok(()));
    }

    #[test]
    fn a_letter_arrives_with_its_line_and_a_dirty_line_is_dropped() {
        let t = table();
        let mut w = world();
        w.send_letter(&t, 0, 1, "greeting-reeds", Some("see you at the weir"), 100)
            .unwrap();
        w.send_letter(&t, 2, 1, "greeting-reeds", Some("bad \u{1b}[2J text"), 100)
            .unwrap();
        let got: Vec<&LetterBody> = w
            .social
            .mail
            .iter()
            .filter_map(|d| match &d.body {
                Body::Letter(l) => Some(l),
                _ => None,
            })
            .collect();
        assert_eq!(got[0].line.as_deref(), Some("see you at the weir"));
        assert_eq!(got[1].line, None);
        assert_eq!(got[0].from, "Ash");
    }

    #[test]
    fn letters_are_limited_per_day_per_recipient_and_by_blocking() {
        let t = table();
        let mut w = world();
        for n in ["Dun", "Eel", "Fen", "Gull"] {
            w.register(n, 0);
        }
        // Two unread letters from one sender to one recipient, then no more.
        w.send_letter(&t, 0, 1, "greeting-reeds", None, 100)
            .unwrap();
        w.send_letter(&t, 0, 1, "greeting-reeds", None, 100)
            .unwrap();
        assert_eq!(
            w.send_letter(&t, 0, 1, "greeting-reeds", None, 100),
            Err(Refusal::Unread)
        );
        // Five letters a day in all.
        for to in 2..5 {
            w.send_letter(&t, 0, to, "greeting-reeds", None, 100)
                .unwrap();
        }
        assert_eq!(
            w.send_letter(&t, 0, 5, "greeting-reeds", None, 100),
            Err(Refusal::LettersToday)
        );
        // The next day the count is fresh.
        assert_eq!(w.send_letter(&t, 0, 5, "greeting-reeds", None, 101), Ok(()));
        // A recipient who blocks hears nothing more; the sender is not told.
        w.block(6, 5).unwrap();
        let before = w.social.mail.len();
        assert_eq!(w.send_letter(&t, 5, 6, "greeting-reeds", None, 100), Ok(()));
        assert_eq!(w.social.mail.len(), before);
        assert_eq!(w.block(6, 6), Err(Refusal::Unknown));
        assert_eq!(w.block(6, 99), Err(Refusal::Unknown));
    }

    #[test]
    fn a_taunt_cannot_be_sent_to_a_pact_partner() {
        let t = table();
        let mut w = world();
        w.clans[0].record.raids_won = 2;
        w.clans[0].posted.buildings = 4;
        let id = w.propose(0, 1, crate::social::pact::TRUCE, 100).unwrap();
        w.accept(1, id, 100).unwrap();
        assert_eq!(
            w.send_letter(&t, 0, 1, "taunt-keeps", None, 100),
            Err(Refusal::Pact)
        );
        assert_eq!(w.send_letter(&t, 0, 2, "taunt-keeps", None, 100), Ok(()));
    }

    #[test]
    fn a_letter_round_trips() {
        let l = LetterBody {
            from: "Ash".into(),
            template: "greeting-reeds".into(),
            line: Some("hello".into()),
            day: 9,
        };
        let mut w = Writer::new();
        l.write(&mut w);
        let bytes = w.finish();
        assert_eq!(LetterBody::read(&mut Reader::new(&bytes)), Some(l));
    }
}
