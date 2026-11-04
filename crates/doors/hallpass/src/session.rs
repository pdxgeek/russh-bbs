//! The text screen: one view that is both the drawing and the look-around.
//!
//! The screen is a thin layer. Every choice is an [`Action`] the engine decides; the screen only
//! lists the legal ones as `[key] label` lines and turns a key into the action. A bot (or a screen
//! reader) can read the same lines.

use std::sync::Arc;

use russh_bbs_door::{Canvas, Ctx, DoorEnv, DoorKey, DoorSession, DoorStyle, Flow};

use crate::content::Content;
use crate::game::{Action, Event, Game};
use crate::save::encode_player;
use crate::state::Status;
use crate::world::WorldDelta;

/// Narrow enough for the door area inside the board's frame on an 80-column terminal.
const WIDTH: usize = 70;

/// Which screen is up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Mode {
    Main,
    Go,
    Study,
    Shop,
    Plans,
    DraftTarget,
    DraftMethod(String),
    Scheme(u16),
    Twists(u16),
    Getaway(u16),
    Recruit(u16),
    Schedule(u16),
    Stats,
    News,
}

#[derive(Debug, Clone)]
enum Cmd {
    Do(Action),
    To(Mode),
    Leave,
    NextPage,
    PrevPage,
}

struct Item {
    key: char,
    label: String,
    cmd: Cmd,
}

pub(crate) struct Session {
    game: Game,
    mode: Mode,
    page: usize,
    messages: Vec<String>,
    dirty: bool,
}

fn wrap(text: &str, width: usize) -> Vec<String> {
    let indent: String = text.chars().take_while(|c| *c == ' ').collect();
    let mut lines = Vec::new();
    let mut line = indent.clone();
    for word in text.split_whitespace() {
        let started = line.len() > indent.len();
        if started && line.chars().count() + 1 + word.chars().count() > width {
            lines.push(std::mem::replace(&mut line, indent.clone()));
        }
        if line.len() > indent.len() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if line.len() > indent.len() {
        lines.push(line);
    }
    lines
}

const DIGITS: [char; 9] = ['1', '2', '3', '4', '5', '6', '7', '8', '9'];

impl Session {
    pub(crate) fn new(mut game: Game, opening: Vec<Event>) -> Self {
        let dirty = !opening.is_empty();
        let mut messages: Vec<String> = Vec::new();
        for event in opening {
            messages.extend(wrap(&event.describe(), WIDTH));
        }
        // Whatever the morning did to the world is sent with the first key.
        let _ = game.take_world_changes();
        Self {
            game,
            mode: Mode::Main,
            page: 0,
            messages,
            dirty,
        }
    }

    fn content(&self) -> &Content {
        &self.game.content
    }

    // ------------------------------------------------------------ acting

    fn act(&mut self, action: &Action, ctx: &mut Ctx<'_>) {
        match self.game.apply(action, ctx.rng()) {
            Ok(events) => {
                self.messages.clear();
                for event in &events {
                    self.messages.extend(wrap(&event.describe(), WIDTH));
                }
                self.dirty = true;
                self.flush_world(ctx);
                if matches!(action, Action::Draft { .. }) {
                    if let Some(s) = self.game.player.schemes.last() {
                        self.mode = Mode::Scheme(s.id);
                    }
                } else if matches!(action, Action::Drop(_)) {
                    self.mode = Mode::Plans;
                } else if matches!(action, Action::Go(_) | Action::Study(_)) {
                    self.mode = Mode::Main;
                }
            }
            Err(refused) => self.messages = wrap(&refused.0, WIDTH),
        }
    }

    fn flush_world(&mut self, ctx: &mut Ctx<'_>) {
        let today = self.game.player.day;
        for delta in self.game.take_world_changes() {
            let content = Arc::clone(&self.game.content);
            ctx.edit_world(move |bytes| {
                Some(WorldDelta::apply_to_bytes(&delta, bytes, today, &content))
            });
        }
    }

    // ------------------------------------------------------------ building the screen

    fn status_lines(&self) -> [String; 2] {
        let g = &self.game;
        let p = &g.player;
        let slot = g.current_slot().map_or_else(
            || "lights out".to_owned(),
            |s| {
                g.content
                    .periods
                    .get(s)
                    .map_or("?", |x| x.name.as_str())
                    .to_owned()
            },
        );
        [
            format!(
                "Term {} - day {} of {} - {}",
                p.term,
                (g.term_day() + 1).max(1),
                g.content.rules.term_days,
                slot
            ),
            format!(
                "heat {}  file {}/{}  legend {}  nerve {}/{}  cash {}  walk {}",
                p.heat,
                p.file_points(),
                g.content.rules.file_full,
                p.legend,
                p.nerve,
                p.nerve_max,
                p.cash,
                p.moves
            ),
        ]
    }

    fn timetable_strip(&self) -> String {
        let g = &self.game;
        let day = g.player.day;
        let now = g.current_slot();
        (0..g.periods())
            .map(|slot| {
                let act = g.activity(day, slot);
                let short: String = match act.strip_prefix("class:") {
                    Some(sub) => {
                        let mut it = sub.chars();
                        it.next()
                            .map(|c| c.to_ascii_uppercase())
                            .into_iter()
                            .chain(it.take(2))
                            .collect()
                    }
                    None => match act {
                        "lunch" => "Lunch".to_owned(),
                        "free" => "Free".to_owned(),
                        "after" => "After".to_owned(),
                        "evening" => "Eve".to_owned(),
                        "night" => "Night".to_owned(),
                        other => other.chars().take(5).collect(),
                    },
                };
                if now == Some(slot) {
                    format!("[{short}]")
                } else {
                    short
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Whether the engine would accept `action` now.
    fn can(&self, action: &Action) -> bool {
        self.game.can(action)
    }

    /// The screen: header lines, body lines, and the menu.
    fn view(&self) -> (Vec<String>, Vec<Item>) {
        let g = &self.game;
        if g.player.status == Status::New {
            return self.enrol_view();
        }
        if !g.playing() {
            return self.over_view();
        }
        if g.player.run.is_some() {
            return self.run_view();
        }
        match &self.mode {
            Mode::Main => self.main_view(),
            Mode::Go => self.go_view(),
            Mode::Study => self.study_view(),
            Mode::Shop => self.shop_view(),
            Mode::Plans => self.plans_view(),
            Mode::DraftTarget => self.draft_target_view(),
            Mode::DraftMethod(target) => self.draft_method_view(target),
            Mode::Scheme(id) => self.scheme_view(*id),
            Mode::Twists(id) => self.twists_view(*id),
            Mode::Getaway(id) => self.getaway_view(*id),
            Mode::Recruit(id) => self.recruit_view(*id),
            Mode::Schedule(id) => self.schedule_view(*id),
            Mode::Stats => self.stats_view(),
            Mode::News => self.news_view(),
        }
    }

    fn back(label: &str) -> Item {
        Item {
            key: 'b',
            label: label.to_owned(),
            cmd: Cmd::To(Mode::Main),
        }
    }

    fn paged(&self, mut items: Vec<Item>, back: Item) -> Vec<Item> {
        let per = DIGITS.len();
        let pages = items.len().div_ceil(per).max(1);
        let page = self.page.min(pages - 1);
        let mut shown: Vec<Item> = items.drain(..).skip(page * per).take(per).collect();
        for (item, key) in shown.iter_mut().zip(DIGITS) {
            item.key = key;
        }
        if page + 1 < pages {
            shown.push(Item {
                key: '.',
                label: "More".into(),
                cmd: Cmd::NextPage,
            });
        }
        if page > 0 {
            shown.push(Item {
                key: ',',
                label: "Previous".into(),
                cmd: Cmd::PrevPage,
            });
        }
        shown.push(back);
        shown
    }

    fn enrol_view(&self) -> (Vec<String>, Vec<Item>) {
        let mut body = vec![
            "GREYWATER ACADEMY".to_owned(),
            "A boarding school on a fog-bound lake, too many locked doors, and a headmistress with a file on everybody.".to_owned(),
            "You arrive mid-year. Who do you want to be known as?".to_owned(),
        ];
        let mut items = Vec::new();
        for (i, t) in self.content().traits.iter().filter(|t| t.start).enumerate() {
            body.push(format!("{}. {}: {}", i + 1, t.name, t.blurb));
            items.push(Item {
                key: DIGITS.get(i).copied().unwrap_or('9'),
                label: t.name.clone(),
                cmd: Cmd::Do(Action::Enrol(t.id.clone())),
            });
        }
        items.push(Item {
            key: 'q',
            label: "Leave".into(),
            cmd: Cmd::Leave,
        });
        (body.iter().flat_map(|l| wrap(l, WIDTH)).collect(), items)
    }

    fn over_view(&self) -> (Vec<String>, Vec<Item>) {
        let g = &self.game;
        let mut body = Vec::new();
        let p = &g.player;
        let result = g.latest_result();
        if p.status == Status::Expelled {
            body.push("You have been expelled. The term is over for you.".to_owned());
        } else if p.status == Status::Finished || g.term_over() {
            body.push("The term is over.".to_owned());
        }
        if let Some(r) = result {
            if let Some(e) = g.content.endings.iter().find(|e| e.id == r.ending) {
                body.push(format!("{}: {}", e.name, e.text));
            }
            body.push(format!(
                "Legend {}, file {}, average grade {}.",
                r.legend, r.file, r.grades
            ));
        }
        body.push("A new term starts when the school year turns over. Come back then.".to_owned());
        body.extend(self.hall_lines());
        (
            body.iter().flat_map(|l| wrap(l, WIDTH)).collect(),
            vec![Item {
                key: 'q',
                label: "Leave".into(),
                cmd: Cmd::Leave,
            }],
        )
    }

    fn hall_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        if !self.game.world.hall.is_empty() {
            out.push("Hall of legend:".to_owned());
            for (i, e) in self.game.world.hall.iter().take(5).enumerate() {
                out.push(format!("  {}. {} - {}", i + 1, e.name, e.legend));
            }
        }
        for y in self.game.world.yearbook.iter().rev().take(2) {
            out.push(format!(
                "Yearbook, term {}: {} ({} legend, {} changes to the school)",
                y.term, y.name, y.legend, y.changes
            ));
        }
        out
    }

    fn main_view(&self) -> (Vec<String>, Vec<Item>) {
        let g = &self.game;
        let mut body = Vec::new();
        body.push(self.timetable_strip());
        let slot = g.current_slot();
        let place = g.content.place(&g.player.place);
        body.push(format!(
            "{} ({})",
            place.map_or("?", |p| p.name.as_str()),
            place.map_or("?", |p| p.wing.as_str())
        ));
        body.extend(wrap(&g.describe_place(&g.player.place), WIDTH));
        if let Some(slot) = slot {
            let here: Vec<String> = g
                .npcs_at(&g.player.place, g.player.day, slot)
                .iter()
                .map(|n| format!("{} ({})", n.name, n.title))
                .collect();
            if !here.is_empty() {
                body.extend(wrap(&format!("Here: {}.", here.join("; ")), WIDTH));
            }
            let act = g.activity(g.player.day, slot);
            body.push(match act.strip_prefix("class:") {
                Some(s) => format!(
                    "This period: {} class.",
                    g.content.subject(s).map_or(s, |x| x.name.as_str())
                ),
                None => format!("This period: {act}."),
            });
        } else {
            body.push("Lights are out. The day is done; come back tomorrow.".to_owned());
        }
        if g.player.detention > 0 {
            body.push(format!("Detention owed: {}.", g.player.detention));
        }
        if g.player.summons {
            body.push("You are summoned to the headmistress tomorrow.".to_owned());
        }
        let shop_items: Vec<Action> = g
            .content
            .items
            .iter()
            .filter(|i| {
                g.content
                    .place(&g.player.place)
                    .and_then(|p| p.shop.as_deref())
                    == Some(i.shop.as_str())
            })
            .map(|i| Action::Buy(i.id.clone()))
            .collect();
        let mut items = Vec::new();
        let mut add = |key: char, label: &str, cmd: Cmd| {
            items.push(Item {
                key,
                label: label.to_owned(),
                cmd,
            });
        };
        if !g.reachable().is_empty() {
            add('g', "Go somewhere", Cmd::To(Mode::Go));
        }
        if self.can(&Action::Attend) {
            add('a', "Attend (class or detention)", Cmd::Do(Action::Attend));
        }
        if g.content
            .subjects
            .first()
            .is_some_and(|s| self.can(&Action::Study(s.id.clone())))
        {
            add('s', "Study", Cmd::To(Mode::Study));
        }
        if shop_items.iter().any(|a| self.can(a)) {
            add('b', "Buy something", Cmd::To(Mode::Shop));
        }
        if self.can(&Action::Loiter) {
            add('l', "Loiter and listen", Cmd::Do(Action::Loiter));
        }
        if self.can(&Action::Rest) {
            add('r', "Rest, head down", Cmd::Do(Action::Rest));
        }
        add('p', "Schemes", Cmd::To(Mode::Plans));
        add('f', "You: file, grades, standing", Cmd::To(Mode::Stats));
        add('n', "News and rumours", Cmd::To(Mode::News));
        if self.can(&Action::Sleep) {
            add('z', "Lights out", Cmd::Do(Action::Sleep));
        }
        add('q', "Leave the school", Cmd::Leave);
        (body, items)
    }

    fn go_view(&self) -> (Vec<String>, Vec<Item>) {
        let mut places = self.game.reachable();
        places.sort_by_key(|p| {
            self.content()
                .place(p)
                .map_or(p.clone(), |x| x.name.clone())
        });
        let items: Vec<Item> = places
            .into_iter()
            .map(|p| Item {
                label: self
                    .content()
                    .place(&p)
                    .map_or(p.clone(), |x| format!("{} ({})", x.name, x.wing)),
                key: ' ',
                cmd: Cmd::Do(Action::Go(p)),
            })
            .collect();
        (
            vec!["Where to? (a few corridors at most each period)".to_owned()],
            self.paged(items, Self::back("Back")),
        )
    }

    fn study_view(&self) -> (Vec<String>, Vec<Item>) {
        let items: Vec<Item> = self
            .content()
            .subjects
            .iter()
            .map(|s| Item {
                key: ' ',
                label: format!(
                    "{} (now {})",
                    s.name,
                    crate::state::map_get(&self.game.player.grades, &s.id)
                        .copied()
                        .unwrap_or(0)
                ),
                cmd: Cmd::Do(Action::Study(s.id.clone())),
            })
            .collect();
        (
            vec!["Study which subject?".to_owned()],
            self.paged(items, Self::back("Back")),
        )
    }

    fn shop_view(&self) -> (Vec<String>, Vec<Item>) {
        let g = &self.game;
        let shop = g
            .content
            .place(&g.player.place)
            .and_then(|p| p.shop.clone());
        let items: Vec<Item> = g
            .content
            .items
            .iter()
            .filter(|i| shop.as_deref() == Some(i.shop.as_str()))
            .map(|i| Item {
                key: ' ',
                label: format!(
                    "{} - {} cash{}",
                    i.name,
                    i.price,
                    if g.all_hold(&i.needs) {
                        ""
                    } else {
                        " (not for you yet)"
                    }
                ),
                cmd: Cmd::Do(Action::Buy(i.id.clone())),
            })
            .collect();
        let have: Vec<String> = g
            .player
            .inventory
            .iter()
            .map(|(i, n)| {
                format!(
                    "{}x {}",
                    n,
                    g.content.item(i).map_or(i.as_str(), |x| x.name.as_str())
                )
            })
            .collect();
        let carried = if have.is_empty() {
            "You carry nothing useful.".to_owned()
        } else {
            format!("You carry: {}.", have.join(", "))
        };
        (
            vec![format!("You have {} cash.", g.player.cash), carried],
            self.paged(items, Self::back("Back")),
        )
    }

    fn plans_view(&self) -> (Vec<String>, Vec<Item>) {
        let g = &self.game;
        let mut body = vec![
            "Your schemes. Mischief against authority and rivals: that is the game.".to_owned(),
        ];
        if let Some(c) = &g.player.campaign {
            body.push(format!(
                "Campaign: {} link(s) against {}.",
                c.links,
                g.content.target(&c.theme).map_or("?", |t| t.name.as_str())
            ));
        }
        let mut items: Vec<Item> = g
            .player
            .schemes
            .iter()
            .map(|s| Item {
                key: ' ',
                label: self.scheme_title(s.id),
                cmd: Cmd::To(Mode::Scheme(s.id)),
            })
            .collect();
        if g.player.schemes.len() < g.content.rules.max_schemes {
            items.push(Item {
                key: ' ',
                label: "Start a new scheme".into(),
                cmd: Cmd::To(Mode::DraftTarget),
            });
        }
        (body, self.paged(items, Self::back("Back")))
    }

    fn scheme_title(&self, id: u16) -> String {
        let g = &self.game;
        let Some(s) = g.player.scheme(id) else {
            return String::new();
        };
        let m = g.content.method(&s.method).map_or("?", |m| m.name.as_str());
        let t = g.content.target(&s.target).map_or("?", |t| t.name.as_str());
        let state = if g.player.run.as_ref().is_some_and(|r| r.scheme == id) {
            "RUNNING"
        } else if g.readiness(s).ready() {
            "READY"
        } else {
            "setup"
        };
        format!("#{id} {m} - {t} [{state}]")
    }

    fn draft_target_view(&self) -> (Vec<String>, Vec<Item>) {
        let g = &self.game;
        let drafts = g.drafts();
        let mut ids: Vec<String> = Vec::new();
        for (t, _) in &drafts {
            if !ids.contains(t) {
                ids.push(t.clone());
            }
        }
        let items: Vec<Item> = ids
            .into_iter()
            .map(|t| {
                let wary = g
                    .content
                    .target(&t)
                    .and_then(|x| x.npc.clone())
                    .map_or(0, |n| g.player.wary_of(&n));
                let name = g
                    .content
                    .target(&t)
                    .map_or(t.clone(), |x| format!("{} ({})", x.name, x.kind));
                Item {
                    key: ' ',
                    label: if wary > 0 {
                        format!("{name} - wary {wary}")
                    } else {
                        name
                    },
                    cmd: Cmd::To(Mode::DraftMethod(t)),
                }
            })
            .collect();
        (
            vec!["Who or what is it aimed at? (Nobody is ever named by typing.)".to_owned()],
            self.paged(
                items,
                Item {
                    key: 'b',
                    label: "Back".into(),
                    cmd: Cmd::To(Mode::Plans),
                },
            ),
        )
    }

    fn draft_method_view(&self, target: &str) -> (Vec<String>, Vec<Item>) {
        let g = &self.game;
        let items: Vec<Item> = g
            .drafts()
            .into_iter()
            .filter(|(t, _)| t == target)
            .filter_map(|(_, m)| g.content.method(&m).cloned())
            .map(|m| Item {
                key: ' ',
                label: format!(
                    "{}{}",
                    m.name,
                    if m.big {
                        " (BIG: changes the school)"
                    } else {
                        ""
                    }
                ),
                cmd: Cmd::Do(Action::Draft {
                    target: target.to_owned(),
                    method: m.id,
                }),
            })
            .collect();
        let name = g.content.target(target).map_or("?", |t| t.name.as_str());
        (
            vec![format!("What will you do to {name}?")],
            self.paged(
                items,
                Item {
                    key: 'b',
                    label: "Back".into(),
                    cmd: Cmd::To(Mode::DraftTarget),
                },
            ),
        )
    }

    #[allow(clippy::many_single_char_names)] // g (game), s (scheme), m (method), t (target), r (readiness)
    fn scheme_view(&self, id: u16) -> (Vec<String>, Vec<Item>) {
        let g = &self.game;
        let Some(s) = g.player.scheme(id) else {
            return self.plans_view();
        };
        let (Some(m), Some(t)) = (g.content.method(&s.method), g.content.target(&s.target)) else {
            return self.plans_view();
        };
        let r = g.readiness(s);
        let place = g
            .scheme_place(s)
            .and_then(|p| g.content.place(&p).map(|x| x.name.clone()))
            .unwrap_or_default();
        let mut body = vec![
            format!("#{id} {} - {}", m.name, t.name),
            m.blurb.clone(),
            format!("Where: {place}."),
        ];
        let twists: Vec<&str> = s
            .twists
            .iter()
            .filter_map(|x| g.content.twist(x))
            .map(|x| x.name.as_str())
            .collect();
        body.push(format!(
            "Twists: {}. Getaway: {}.",
            if twists.is_empty() {
                "none".to_owned()
            } else {
                twists.join(", ")
            },
            g.content
                .getaway(&s.getaway)
                .map_or("?", |x| x.name.as_str())
        ));
        let items_line: Vec<String> = m
            .needs
            .iter()
            .map(|i| {
                format!(
                    "{} ({})",
                    g.content.item(i).map_or(i.as_str(), |x| x.name.as_str()),
                    if g.player.count(i) > 0 {
                        "have"
                    } else {
                        "NEED"
                    }
                )
            })
            .collect();
        if !items_line.is_empty() {
            body.push(format!("Items: {}.", items_line.join("; ")));
        }
        let roles: Vec<String> = g
            .needed_roles(s)
            .into_iter()
            .map(|role| match s.allies.iter().find(|(_, r)| *r == role) {
                Some((n, _)) => format!(
                    "{role}: {}",
                    g.content.npc(n).map_or("?", |x| x.name.as_str())
                ),
                None => format!("{role}: NEED"),
            })
            .collect();
        if !roles.is_empty() {
            body.push(format!("Helpers: {}.", roles.join("; ")));
        }
        let extra: Vec<String> = s
            .allies
            .iter()
            .filter(|(_, role)| !g.needed_roles(s).contains(role))
            .map(|(n, role)| {
                format!(
                    "{role}: {}",
                    g.content.npc(n).map_or("?", |x| x.name.as_str())
                )
            })
            .collect();
        if !extra.is_empty() {
            body.push(format!("Also helping: {}.", extra.join("; ")));
        }
        if let Some(lock) = g
            .scheme_place(s)
            .and_then(|p| g.content.place(&p))
            .filter(|p| p.lock.is_some())
        {
            let open = lock.unlock.iter().any(|c| g.holds(c));
            let ways: Vec<String> = lock
                .unlock
                .iter()
                .map(crate::cond::Cond::describe)
                .collect();
            body.push(if open {
                "The way in: open to you.".to_owned()
            } else {
                format!("The way in is locked. Any of: {}.", ways.join("; "))
            });
        }
        body.push(format!(
            "Scouting {}/{}. {}",
            r.scout_have,
            r.scout_need,
            s.when.map_or("No time set.".to_owned(), |(d, sl)| format!(
                "Set for {} (day {}), period {}.",
                ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"][Game::weekday(d)],
                d - g.world.term_start + 1,
                sl + 1
            ))
        ));
        let odds: Vec<String> = [
            crate::game::Phase::Sneak,
            crate::game::Phase::Act,
            crate::game::Phase::Escape,
        ]
        .iter()
        .filter_map(|p| {
            g.chance(s, *p, 0, 0)
                .map(|c| format!("{} {}%", p.name(), c.total))
        })
        .collect();
        body.push(format!(
            "Odds: {}. Traced back if clean: {}%.",
            odds.join(", "),
            g.trace_chance(s, 0)
        ));
        let mut items = vec![
            Item {
                key: 't',
                label: "Twists".into(),
                cmd: Cmd::To(Mode::Twists(id)),
            },
            Item {
                key: 'y',
                label: "Getaway".into(),
                cmd: Cmd::To(Mode::Getaway(id)),
            },
        ];
        if !g.recruitable(s).is_empty() {
            items.push(Item {
                key: 'r',
                label: "Recruit someone here".into(),
                cmd: Cmd::To(Mode::Recruit(id)),
            });
        }
        if self.can(&Action::Scout(id)) {
            items.push(Item {
                key: 'w',
                label: "Scout (takes the period)".into(),
                cmd: Cmd::Do(Action::Scout(id)),
            });
        }
        items.push(Item {
            key: 'd',
            label: "Set the day and period".into(),
            cmd: Cmd::To(Mode::Schedule(id)),
        });
        if self.can(&Action::BeginRun(id)) {
            items.push(Item {
                key: 'x',
                label: "GO: run it now".into(),
                cmd: Cmd::Do(Action::BeginRun(id)),
            });
        }
        items.push(Item {
            key: 'z',
            label: "Tear it up".into(),
            cmd: Cmd::Do(Action::Drop(id)),
        });
        items.push(Item {
            key: 'b',
            label: "Back".into(),
            cmd: Cmd::To(Mode::Plans),
        });
        (body, items)
    }

    fn twists_view(&self, id: u16) -> (Vec<String>, Vec<Item>) {
        let g = &self.game;
        let Some(s) = g.player.scheme(id) else {
            return self.plans_view();
        };
        let mut items = Vec::new();
        for t in &g.content.twists {
            let on = s.twists.contains(&t.id);
            let available = on || g.all_hold(&t.requires);
            if !available {
                continue;
            }
            let action = if on {
                Action::RemoveTwist(id, t.id.clone())
            } else {
                Action::AddTwist(id, t.id.clone())
            };
            items.push(Item {
                key: ' ',
                label: format!(
                    "{} {} - {}",
                    if on { "[x]" } else { "[ ]" },
                    t.name,
                    t.blurb
                ),
                cmd: Cmd::Do(action),
            });
        }
        (
            vec![format!(
                "Twists for #{id} (up to {}). Each changes the odds, the legend and the heat.",
                g.content.rules.max_twists
            )],
            self.paged(
                items,
                Item {
                    key: 'b',
                    label: "Back".into(),
                    cmd: Cmd::To(Mode::Scheme(id)),
                },
            ),
        )
    }

    fn getaway_view(&self, id: u16) -> (Vec<String>, Vec<Item>) {
        let g = &self.game;
        let Some(s) = g.player.scheme(id) else {
            return self.plans_view();
        };
        let items: Vec<Item> = g
            .content
            .getaways
            .iter()
            .filter(|x| g.all_hold(&x.requires))
            .map(|x| Item {
                key: ' ',
                label: format!(
                    "{}{} - {}",
                    if s.getaway == x.id { "* " } else { "" },
                    x.name,
                    x.blurb
                ),
                cmd: Cmd::Do(Action::SetGetaway(id, x.id.clone())),
            })
            .collect();
        (
            vec![format!("How do you get away from #{id}?")],
            self.paged(
                items,
                Item {
                    key: 'b',
                    label: "Back".into(),
                    cmd: Cmd::To(Mode::Scheme(id)),
                },
            ),
        )
    }

    fn recruit_view(&self, id: u16) -> (Vec<String>, Vec<Item>) {
        let g = &self.game;
        let Some(s) = g.player.scheme(id) else {
            return self.plans_view();
        };
        let items: Vec<Item> = g
            .recruitable(s)
            .into_iter()
            .map(|(npc, role, cost)| Item {
                key: ' ',
                label: format!(
                    "{} as {role} ({} cash)",
                    g.content.npc(&npc).map_or("?", |n| n.name.as_str()),
                    cost
                ),
                cmd: Cmd::Do(Action::Recruit {
                    scheme: id,
                    npc,
                    role,
                }),
            })
            .collect();
        (
            vec!["Who will help? Asking takes the period.".to_owned()],
            self.paged(
                items,
                Item {
                    key: 'b',
                    label: "Back".into(),
                    cmd: Cmd::To(Mode::Scheme(id)),
                },
            ),
        )
    }

    fn schedule_view(&self, id: u16) -> (Vec<String>, Vec<Item>) {
        let g = &self.game;
        let Some(s) = g.player.scheme(id) else {
            return self.plans_view();
        };
        // Scouting is what tells you who will be about.
        let place = g.scheme_place(s).unwrap_or_default();
        let watching = |s: &crate::state::Scheme, day: i64, slot: u8| -> String {
            if s.scout == 0 {
                String::new()
            } else {
                format!(
                    ", {} watching",
                    g.witnesses_at(&place, day, usize::from(slot))
                )
            }
        };
        let items: Vec<Item> = g
            .candidate_times(s)
            .into_iter()
            .map(|(day, slot)| Item {
                key: ' ',
                label: format!(
                    "{} (day {}), {}: {}{}",
                    ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"][Game::weekday(day)],
                    day - g.world.term_start + 1,
                    g.content
                        .periods
                        .get(usize::from(slot))
                        .map_or("?", |p| p.name.as_str()),
                    g.activity(day, usize::from(slot)),
                    watching(s, day, slot)
                ),
                cmd: Cmd::Do(Action::Schedule {
                    scheme: id,
                    day,
                    slot,
                }),
            })
            .collect();
        let mut body = vec!["When? These are the periods where the place is open and the target is where the trick needs them.".to_owned()];
        if s.scout > 0
            && let Some(npc) = g
                .content
                .target(&s.target)
                .and_then(|t| t.npc.clone())
                .and_then(|n| g.content.npc(&n).cloned())
        {
            body.push(format!(
                "Scouted: {} keeps to a routine you now know.",
                npc.name
            ));
        }
        (
            body,
            self.paged(
                items,
                Item {
                    key: 'b',
                    label: "Back".into(),
                    cmd: Cmd::To(Mode::Scheme(id)),
                },
            ),
        )
    }

    fn stats_view(&self) -> (Vec<String>, Vec<Item>) {
        let g = &self.game;
        let p = &g.player;
        let mut body = Vec::new();
        let traits: Vec<&str> = p
            .traits
            .iter()
            .filter_map(|t| g.content.trait_spec(t))
            .map(|t| t.name.as_str())
            .collect();
        body.push(format!("{}  Traits: {}.", p.name, traits.join(", ")));
        body.push(format!(
            "Cool {}  Notoriety {}  Heat {}  Nerve {}/{}",
            p.cool, p.notoriety, p.heat, p.nerve, p.nerve_max
        ));
        let skills: Vec<String> = g
            .content
            .skills
            .iter()
            .map(|s| format!("{} {}", s.name, g.skill_level(&s.id)))
            .collect();
        body.push(format!("Skills (grow with use): {}", skills.join(", ")));
        let grades: Vec<String> = g
            .content
            .subjects
            .iter()
            .map(|s| {
                format!(
                    "{} {}",
                    s.name,
                    crate::state::map_get(&p.grades, &s.id)
                        .copied()
                        .unwrap_or(0)
                )
            })
            .collect();
        body.push(format!("Grades: {}", grades.join(", ")));
        let standing: Vec<String> = g
            .content
            .cliques
            .iter()
            .map(|c| {
                format!(
                    "{} {}",
                    c.name.trim_start_matches("The "),
                    p.standing_with(&c.id)
                )
            })
            .collect();
        body.push(format!("Standing: {}", standing.join(", ")));
        let wary: Vec<String> = p
            .wary
            .iter()
            .filter_map(|(n, w)| g.content.npc(n).map(|x| format!("{} {}", x.name, w.level)))
            .collect();
        if !wary.is_empty() {
            body.push(format!("Watchful of you: {}", wary.join(", ")));
        }
        body.push(format!(
            "File {}/{} (a week of good conduct strikes a point). Detention owed {}.",
            p.file_points(),
            g.content.rules.file_full,
            p.detention
        ));
        body.extend(g.file_text().into_iter().map(|l| format!("  - {l}")));
        body.push(format!(
            "Pulled off {}, caught {}, traced {}, big {}.",
            p.pulled, p.caught, p.pinned, p.big
        ));
        (
            body.iter().flat_map(|l| wrap(l, WIDTH)).collect(),
            vec![Self::back("Back")],
        )
    }

    fn news_view(&self) -> (Vec<String>, Vec<Item>) {
        let g = &self.game;
        let mut body = vec!["The school paper:".to_owned()];
        if g.world.paper.is_empty() {
            body.push("  (nothing yet)".to_owned());
        }
        for h in g.world.paper.iter().rev().take(5) {
            body.push(format!("  - {}", h.text));
        }
        for a in &g.world.active {
            if let Some(e) = g.content.effect(&a.id) {
                body.push(format!("In force: {}.", e.name));
            }
        }
        body.extend(self.hall_lines());
        for line in g.player.news.iter().rev().take(3) {
            body.push(format!("Earlier: {line}"));
        }
        (
            body.iter().flat_map(|l| wrap(l, WIDTH)).collect(),
            vec![Self::back("Back")],
        )
    }

    fn run_view(&self) -> (Vec<String>, Vec<Item>) {
        let g = &self.game;
        let (Some(run),) = (g.player.run.clone(),) else {
            return self.main_view();
        };
        let Some(s) = g.player.scheme(run.scheme) else {
            return self.main_view();
        };
        let phase = crate::game::Phase::from_index(run.phase).unwrap_or(crate::game::Phase::Sneak);
        let mut body = vec![format!(
            "{} - phase: {}",
            self.scheme_title(run.scheme),
            phase.name().to_uppercase()
        )];
        if let Some(c) = g.chance(s, phase, 0, run.flubs) {
            body.push(format!(
                "Odds {}% (each point of nerve adds {}%).",
                c.total, g.content.rules.nerve_push_bonus
            ));
            let why: Vec<String> = c
                .parts
                .iter()
                .filter(|(_, v)| *v != 0)
                .map(|(n, v)| format!("{n} {v:+}"))
                .collect();
            body.extend(wrap(&why.join(", "), WIDTH));
        }
        body.push(format!(
            "Nerve left: {}. Slips so far: {}.",
            g.player.nerve, run.flubs
        ));
        let items = (0..=3u8)
            .filter(|p| *p <= g.player.nerve)
            .map(|p| Item {
                key: char::from(b'0' + p),
                label: format!("Go, spending {p} nerve"),
                cmd: Cmd::Do(Action::RunPhase { push: p }),
            })
            .collect();
        (body, items)
    }

    // ------------------------------------------------------------ drawing

    fn lines(&self, height: usize, columns: bool) -> Vec<(String, DoorStyle)> {
        let (body, items) = self.view();
        let mut body: Vec<String> = body.iter().flat_map(|l| wrap(l, WIDTH)).collect();
        let mut top = vec![
            ("HALLPASS - Greywater Academy".to_owned(), DoorStyle::Strong),
            (self.status_lines()[0].clone(), DoorStyle::Dim),
            (self.status_lines()[1].clone(), DoorStyle::Dim),
            ("-".repeat(WIDTH), DoorStyle::Dim),
        ];
        let mut tail: Vec<(String, DoorStyle)> = Vec::new();
        if !self.messages.is_empty() {
            tail.push(("-".repeat(WIDTH), DoorStyle::Dim));
            tail.extend(
                self.messages
                    .iter()
                    .take(6)
                    .map(|m| (m.clone(), DoorStyle::Accent)),
            );
        }
        tail.push(("-".repeat(WIDTH), DoorStyle::Dim));
        let cells: Vec<String> = items
            .iter()
            .map(|i| format!("[{}] {}", i.key, i.label))
            .collect();
        if columns && cells.len() > 6 && cells.iter().all(|c| c.chars().count() <= 33) {
            let rows = cells.len().div_ceil(2);
            for row in 0..rows {
                let left = cells.get(row).cloned().unwrap_or_default();
                let right = cells.get(row + rows).cloned().unwrap_or_default();
                tail.push((format!("{left:<35}{right}"), DoorStyle::Plain));
            }
        } else {
            tail.extend(cells.into_iter().map(|c| (c, DoorStyle::Plain)));
        }
        let room = height.saturating_sub(top.len() + tail.len());
        if body.len() > room {
            body.truncate(room.saturating_sub(1));
            body.push("...".to_owned());
        }
        top.extend(body.into_iter().map(|l| (l, DoorStyle::Plain)));
        top.extend(tail);
        top
    }
}

impl DoorSession for Session {
    fn key(&mut self, key: DoorKey, ctx: &mut Ctx<'_>) -> Flow {
        let today = ctx.today().as_raw();
        let events = self.game.sync_day(today);
        if !events.is_empty() {
            self.dirty = true;
            self.messages = events
                .iter()
                .flat_map(|e| wrap(&e.describe(), WIDTH))
                .collect();
            let _ = self.game.take_world_changes();
        }
        let (_, items) = self.view();
        let pressed = match key {
            DoorKey::Char(c) => Some(c.to_ascii_lowercase()),
            DoorKey::Escape => Some('b'),
            _ => None,
        };
        let mut flow = Flow::Stay;
        if let Some(c) = pressed {
            let top_level = matches!(self.mode, Mode::Main)
                || !self.game.playing()
                || self.game.player.status == Status::New;
            if let Some(item) = items.iter().find(|i| i.key == c) {
                match item.cmd.clone() {
                    Cmd::Do(action) => self.act(&action, ctx),
                    Cmd::To(mode) => {
                        self.mode = mode;
                        self.page = 0;
                        self.messages.clear();
                    }
                    Cmd::NextPage => self.page += 1,
                    Cmd::PrevPage => self.page = self.page.saturating_sub(1),
                    Cmd::Leave => flow = Flow::Exit,
                }
            } else if matches!(key, DoorKey::Escape) && top_level {
                flow = Flow::Exit;
            }
        }
        if self.dirty {
            ctx.request_save(encode_player(&self.game.player));
            self.dirty = false;
        }
        flow
    }

    fn render(&self, canvas: &mut Canvas<'_>, _env: &DoorEnv) {
        canvas.clear();
        let height = usize::from(canvas.height());
        for (y, (line, style)) in self
            .lines(height, true)
            .into_iter()
            .take(height)
            .enumerate()
        {
            canvas.text(0, u16::try_from(y).unwrap_or(u16::MAX), &line, style);
        }
    }

    fn look_around(&self, _env: &DoorEnv) -> Vec<String> {
        self.lines(usize::MAX / 2, false)
            .into_iter()
            .map(|(line, _)| line)
            .collect()
    }
}
