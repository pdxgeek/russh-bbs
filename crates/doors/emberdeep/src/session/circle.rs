//! The social places as text screens: the company (parties), the duel ring, the journal of the
//! chain of questlines (with the epitaph), and the Cold Furnace's verdict. Like every place, each
//! is a head and a menu of engine actions; no rule lives here.

use russh_bbs_door::DoorStyle;

use super::places::{Entry, Menu, Spot, key_lines};
use super::{Line, Session, WRAP, plain, styled, wrap};
use crate::company::SocialAct;
use crate::game::{Action, Game};
use crate::social::{
    BOUTS_PER_DAY, CALLS_PER_TARGET, DUEL_LEVEL_GAP, EPITAPH_MAX, INVITE_DAYS, PARTY_MAX, WAGERS,
};

/// Heroes listed to invite, and to call out.
const LISTED: usize = 6;

impl Session {
    /// The company: who rides with the hero, who has invited them, and who could be invited.
    pub(super) fn company_body(&self, game: &Game) -> (Vec<Line>, Vec<Entry>) {
        let c = &game.world.commons;
        let me = game.hero.name.as_str();
        let mut head: Vec<Line> = Vec::new();
        let mut menu = Menu::default();
        match c.party_of(me) {
            Some(p) => {
                head.push(styled(
                    format!("Your company ({} of {PARTY_MAX}):", p.members.len()),
                    DoorStyle::Accent,
                ));
                for m in &p.members {
                    let level = c.member(m).map_or(0, |x| x.level);
                    let tag = if *m == p.leader { " (leader)" } else { "" };
                    head.push(plain(format!("  {m}, level {level}{tag}")));
                }
                head.push(plain(format!(
                    "  Deepest together: level {}. Carried home by each other: {}.",
                    p.deepest, p.rescues
                )));
                head.extend(wrap(
                    "Packs a companion dealt with today stay dealt with for you. A companion carries you home for one turn less and with no wound.",
                    WRAP,
                ).into_iter().map(|l| styled(l, DoorStyle::Dim)));
                menu.add("Leave the company", Action::Social(SocialAct::Leave));
            }
            None => head.push(styled("You ride alone.", DoorStyle::Dim)),
        }
        let waiting: Vec<_> = c
            .invites_for(me)
            .filter(|i| game.day - i.day <= INVITE_DAYS)
            .collect();
        if c.party_of(me).is_none() {
            for i in waiting {
                head.push(plain(format!("{} invites you to ride with them.", i.from)));
                menu.add(
                    format!("Accept {}'s invitation", i.from),
                    Action::Social(SocialAct::Accept(i.from.clone())),
                );
            }
        }
        let full = c.party_of(me).is_some_and(|p| p.members.len() >= PARTY_MAX);
        if !full {
            let mut shown = 0;
            for m in &c.roster {
                if m.name == me || !m.open || c.party_of(&m.name).is_some() {
                    continue;
                }
                if c.invites.iter().any(|i| i.from == me && i.to == m.name) {
                    head.push(plain(format!("You have invited {}.", m.name)));
                    continue;
                }
                if shown >= LISTED {
                    break;
                }
                shown += 1;
                menu.add(
                    format!("Invite {} (level {}, {})", m.name, m.level, m.calling),
                    Action::Social(SocialAct::Invite(m.name.clone())),
                );
            }
            if shown == 0 && c.party_of(me).is_none() {
                head.push(styled("No one is looking for company.", DoorStyle::Dim));
            }
        }
        menu.add(
            if game.hero.social.open {
                "Close the door to invitations (opt out)"
            } else {
                "Open the door to invitations"
            },
            Action::Social(SocialAct::Open(!game.hero.social.open)),
        );
        let entries = menu.0;
        (head, entries)
    }

    /// The duel ring: opt-in, capped, with a ladder.
    pub(super) fn ring_body(&self, game: &Game) -> (Vec<Line>, Vec<Entry>) {
        let c = &game.world.commons;
        let me = game.hero.name.as_str();
        let s = &game.hero.social;
        let mut head: Vec<Line> = Vec::new();
        head.extend(wrap(
            "Non-lethal duels against another hero's last known strength. Stakes of 0, 5, 20 or 50 coin: a win returns your stake and half again, a loss goes to the town works. The loser carries a bruise for a day.",
            WRAP,
        ).into_iter().map(plain));
        let mut menu = Menu::default();
        menu.add(
            if s.ring {
                "Step out of the ring"
            } else {
                "Step into the ring (opt in)"
            },
            Action::Social(SocialAct::Ring(!s.ring)),
        );
        if !s.ring {
            head.push(styled(
                "You are not in the ring: no one can call you out, and you cannot call anyone.",
                DoorStyle::Dim,
            ));
            return (head, menu.0);
        }
        head.push(plain(format!(
            "Your record: {} won, {} lost. Duels today: {}/{BOUTS_PER_DAY}.",
            s.wins, s.losses, game.today.bouts
        )));
        let mut shown = 0;
        for m in &c.roster {
            if shown >= 4 {
                break;
            }
            let ok = m.name != me
                && m.ring
                && game.day - m.day <= 14
                && !c.companions(me, &m.name)
                && game.hero.level.abs_diff(m.level) <= DUEL_LEVEL_GAP
                && c.calls_on(&m.name, game.day) < CALLS_PER_TARGET
                && !game.today.sparred.contains(&m.name);
            if !ok || game.today.bouts >= BOUTS_PER_DAY {
                continue;
            }
            shown += 1;
            let rank = c
                .rank_of(&m.name)
                .map_or(String::new(), |r| format!(", {} pts", r.points));
            for wager in WAGERS {
                if wager > game.hero.coin {
                    continue;
                }
                let stake = if wager == 0 {
                    "for pride".to_owned()
                } else {
                    format!("for {wager}c")
                };
                menu.add(
                    format!("Call out {} (level {}{rank}) {stake}", m.name, m.level),
                    Action::Social(SocialAct::Challenge {
                        target: m.name.clone(),
                        wager,
                    }),
                );
            }
        }
        if shown == 0 {
            head.push(styled(
                "No one in range is in the ring today.",
                DoorStyle::Dim,
            ));
        }
        (head, menu.0)
    }

    /// The journal: the chain of questlines, their choices and the epitaph.
    pub(super) fn journal_body(&self, game: &Game) -> (Vec<Line>, Vec<Entry>) {
        let mut head: Vec<Line> = Vec::new();
        let mut menu = Menu::default();
        for q in &self.content.quests.quest {
            if !game.quest_open(q) {
                head.push(styled(
                    format!("{}: opens at level {}.", q.name, q.level),
                    DoorStyle::Dim,
                ));
                continue;
            }
            let done = game.quest_done(&q.id);
            let total = q.step.len();
            if done >= total {
                head.push(styled(
                    format!("{} ({total}/{total}): finished.", q.name),
                    DoorStyle::Good,
                ));
                continue;
            }
            head.push(styled(
                format!("{} ({done}/{total})", q.name),
                DoorStyle::Accent,
            ));
            if let Some(step) = game.quest_step(q) {
                head.extend(
                    wrap(&format!("  {}", step.text), WRAP)
                        .into_iter()
                        .map(plain),
                );
                if game.quest_waiting(q).is_some() {
                    for c in &step.choose {
                        menu.add(
                            c.label.clone(),
                            Action::Social(SocialAct::Choose {
                                quest: q.id.clone(),
                                choice: c.id.clone(),
                            }),
                        );
                    }
                }
            }
        }
        let flags = &game.hero.story.flags;
        if !flags.is_empty() {
            head.push(plain(format!("Choices made: {}", flags.join(", "))));
        }
        head.push(plain(format!(
            "Epitaph (up to {EPITAPH_MAX} characters, read only if you are not found): {}",
            if game.hero.social.epitaph.is_empty() {
                "none"
            } else {
                game.hero.social.epitaph.as_str()
            }
        )));
        head.push(styled("[e] Write an epitaph", DoorStyle::Dim));
        (head, menu.0)
    }

    /// The Cold Furnace asks the one who felled its guardian.
    pub(super) fn verdict_body(&self, game: &Game) -> (Vec<Line>, Vec<Entry>) {
        let mut head: Vec<Line> = Vec::new();
        let mut menu = Menu::default();
        if !game.hero.story.verdict_due {
            head.push(styled("Nothing is asked of you.", DoorStyle::Dim));
            return (head, menu.0);
        }
        head.extend(
            wrap(
                "The guardian is down and the Furnace is open. The fire has not gone out, and it waits on one answer, yours, for everyone in Cindermere.",
                WRAP,
            )
            .into_iter()
            .map(plain),
        );
        let open = game.verdicts();
        for v in &crate::endings::VERDICTS {
            if open.iter().any(|o| o.id == v.id) {
                menu.add(v.label, Action::Social(SocialAct::Verdict(v.id.to_owned())));
            } else {
                head.push(styled(
                    format!(
                        "Closed to you: {} (needs the choice `{}`).",
                        v.label,
                        v.needs_flag.unwrap_or("")
                    ),
                    DoorStyle::Dim,
                ));
            }
        }
        (head, menu.0)
    }
}

impl Session {
    /// What the social place shows (no title, no purse, no keys) and what it offers.
    pub(super) fn social_body(&self, game: &Game, spot: Spot) -> (Vec<Line>, Vec<Entry>) {
        let (lines, entries) = match spot {
            Spot::Company => self.company_body(game),
            Spot::Ring => self.ring_body(game),
            Spot::Journal => self.journal_body(game),
            _ => self.verdict_body(game),
        };
        // Every line fits the board's box.
        let lines = lines
            .into_iter()
            .flat_map(|(text, style)| {
                wrap(&text, WRAP)
                    .into_iter()
                    .map(move |l| (l, style))
                    .collect::<Vec<_>>()
            })
            .collect();
        (lines, entries)
    }

    /// The whole text screen of a social place.
    pub(super) fn social_view(&self, game: &Game, spot: Spot) -> (Vec<Line>, Vec<Entry>) {
        let title = match spot {
            Spot::Company => "THE COMPANY",
            Spot::Ring => "THE DUEL RING",
            Spot::Journal => "THE JOURNAL: the ember-priests' chain",
            _ => "THE COLD FURNACE",
        };
        let (body, entries) = self.social_body(game, spot);
        let mut head = if spot == Spot::Verdict {
            vec![styled(title, DoorStyle::Strong)]
        } else {
            self.head_with_status(game, title)
        };
        head.extend(body);
        head.extend(key_lines(&entries));
        (head, entries)
    }
}
