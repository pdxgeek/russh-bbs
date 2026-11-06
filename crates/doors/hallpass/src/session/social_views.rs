//! The social screens: classmates and the plots that can be played on them, rumours, the school
//! election, and awards with the opt-out. Each is a body and a list of `[key] label` items like
//! every other screen; the engine decides what is allowed (`Game::plot_block`, `rumour_block`,
//! `election_open`), the screen only lists what it said yes to and says why it said no.

use super::{Cmd, Item, Mode, Session, WIDTH, wrap};
use crate::game::Action;
use crate::social::opt_word;

impl Session {
    /// The body wrapped to the width of the panel these screens put it in, so the panel does not
    /// break a line a second time.
    fn wrapped(body: &[String]) -> Vec<String> {
        body.iter().flat_map(|l| wrap(l, WIDTH / 2 - 2)).collect()
    }

    /// The hub: who you are to the others, and where to go.
    pub(super) fn social_view(&self) -> (Vec<String>, Vec<Item>) {
        let g = &self.game;
        let p = &g.player;
        let mut body = vec![format!(
            "Pupils at school: {}. You are {}.",
            g.classmates().len() + 1,
            opt_word(p.social.opt_out)
        )];
        body.push(
            "Nothing here is typed at anyone: every plot and rumour is chosen from a list, and the school can read all of it. Opting out stops plots both ways.".to_owned(),
        );
        for line in p.social.inbox.iter().rev().take(2) {
            body.push(format!("Lately: {line}"));
        }
        if !p.social.awards.is_empty() {
            let titles: Vec<String> = p
                .social
                .awards
                .iter()
                .map(|a| g.award_title(a))
                .collect();
            body.push(format!("Your awards: {}.", titles.join("; ")));
        }
        let mut items = vec![
            Item {
                key: 'c',
                label: "Classmates: play a scheme on one".into(),
                cmd: Cmd::To(Mode::Classmates),
            },
            Item {
                key: 'r',
                label: "Rumours: start one".into(),
                cmd: Cmd::To(Mode::Rumours),
            },
            Item {
                key: 'e',
                label: "The school election".into(),
                cmd: Cmd::To(Mode::Election),
            },
            Item {
                key: 'o',
                label: if p.social.opt_out {
                    "Open yourself to plots again".into()
                } else {
                    "Opt out of plots against you".into()
                },
                cmd: Cmd::Do(Action::OptOut(!p.social.opt_out)),
            },
        ];
        items.push(Self::back("Back"));
        (Self::wrapped(&body), items)
    }

    pub(super) fn classmates_view(&self) -> (Vec<String>, Vec<Item>) {
        let g = &self.game;
        let mates = g.classmates();
        let mut body = vec!["Who? (a name from the roster; you cannot type one)".to_owned()];
        if mates.is_empty() {
            body.push("Nobody else has been at school lately.".to_owned());
        }
        let mut items = Vec::new();
        for m in &mates {
            if m.opted_out {
                body.push(format!("{} has opted out and cannot be reached.", m.name));
                continue;
            }
            items.push(Item {
                key: ' ',
                label: format!("{} ({} legend)", m.name, m.legend),
                cmd: Cmd::To(Mode::PlotKinds(m.name.clone())),
            });
        }
        if g.player.social.opt_out {
            body.push("You have opted out, so you cannot play schemes either.".to_owned());
            items.clear();
        }
        (
            Self::wrapped(&body),
            self.paged(items, Item {
                key: 'b',
                label: "Back".into(),
                cmd: Cmd::To(Mode::Social),
            }),
        )
    }

    pub(super) fn plot_kinds_view(&self, to: &str) -> (Vec<String>, Vec<Item>) {
        let g = &self.game;
        let mut body = vec![format!("A scheme on {to}, from the fixed list:")];
        let mut items = Vec::new();
        for spec in &g.content.social.plots {
            match g.plot_block(&spec.id, to) {
                None => items.push(Item {
                    key: ' ',
                    label: format!("{} (nerve {})", spec.name, spec.nerve),
                    cmd: Cmd::Do(Action::Plot {
                        kind: spec.id.clone(),
                        to: to.to_owned(),
                    }),
                }),
                Some(why) => body.push(format!("{}: {why}", spec.name)),
            }
        }
        (
            Self::wrapped(&body),
            self.paged(items, Item {
                key: 'b',
                label: "Back".into(),
                cmd: Cmd::To(Mode::Classmates),
            }),
        )
    }

    pub(super) fn rumours_view(&self) -> (Vec<String>, Vec<Item>) {
        let g = &self.game;
        let mut body = vec!["Rumours going round:".to_owned()];
        let lines = g.rumour_lines();
        if lines.is_empty() {
            body.push("  (none yet)".to_owned());
        }
        body.extend(lines.into_iter().take(4).map(|l| format!("  - {l}")));
        let mut items = Vec::new();
        for spec in &g.content.social.rumours {
            let ok = g
                .content
                .rumour_subjects(spec)
                .iter()
                .any(|a| g.rumour_block(&spec.id, a).is_none());
            if ok {
                items.push(Item {
                    key: ' ',
                    label: spec.name.clone(),
                    cmd: Cmd::To(Mode::RumourAbout(spec.id.clone())),
                });
            }
        }
        if items.is_empty() {
            body.push("You cannot start one now (you need an audience, and one a day).".to_owned());
        }
        (
            Self::wrapped(&body),
            self.paged(items, Item {
                key: 'b',
                label: "Back".into(),
                cmd: Cmd::To(Mode::Social),
            }),
        )
    }

    pub(super) fn rumour_about_view(&self, id: &str) -> (Vec<String>, Vec<Item>) {
        let g = &self.game;
        let Some(spec) = g.content.rumour(id) else {
            return (Vec::new(), vec![Self::back("Back")]);
        };
        let body = vec![format!("{}: about whom?", spec.name)];
        let items: Vec<Item> = g
            .content
            .rumour_subjects(spec)
            .into_iter()
            .filter(|a| g.rumour_block(id, a).is_none())
            .map(|about| Item {
                key: ' ',
                label: g.subject_name(&about),
                cmd: Cmd::Do(Action::Spread {
                    rumour: id.to_owned(),
                    about,
                }),
            })
            .collect();
        (
            Self::wrapped(&body),
            self.paged(items, Item {
                key: 'b',
                label: "Back".into(),
                cmd: Cmd::To(Mode::Rumours),
            }),
        )
    }

    pub(super) fn election_view(&self) -> (Vec<String>, Vec<Item>) {
        let g = &self.game;
        let rules = &g.content.social.rules;
        let day = g.term_day();
        let mut body = Vec::new();
        match &g.world.commons.election.winner {
            Some(w) => body.push(format!("The election is over: {} won.", g.winner_name(w))),
            None if day < rules.nominate_from => body.push(format!(
                "The election opens for names on day {}; votes from day {}, counted on day {}.",
                rules.nominate_from + 1,
                rules.vote_from + 1,
                rules.tally_day + 1
            )),
            None => body.push(format!(
                "Head of School. Votes from day {}, counted on day {}.",
                rules.vote_from + 1,
                rules.tally_day + 1
            )),
        }
        for b in g.ballot() {
            body.push(format!("{}: {}", b.name, b.platform));
            body.push(format!("   backing {}, votes {}", b.backing, b.votes));
        }
        let mut items = Vec::new();
        let mut add = |label: &str, action: Action| {
            if g.can(&action) {
                items.push(Item {
                    key: ' ',
                    label: label.to_owned(),
                    cmd: Cmd::Do(action),
                });
            }
        };
        add("Stand for election", Action::Nominate);
        add("Canvass the corridors", Action::Canvass);
        for b in g.ballot() {
            add(&format!("Vote for {}", b.name), Action::Vote(b.id.clone()));
        }
        (
            Self::wrapped(&body),
            self.paged(items, Item {
                key: 'b',
                label: "Back".into(),
                cmd: Cmd::To(Mode::Social),
            }),
        )
    }
}
