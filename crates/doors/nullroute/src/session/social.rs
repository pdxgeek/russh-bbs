//! The social screens and their keys (M4-N4): rivals, hacks and bounties; crews and crew runs;
//! the fixers' arcs and their choices; the street word and the hall.
//!
//! **Where the rules live.** None here: the caps are `Social::check_hack` and
//! `Social::check_bounty`, the crew rules are in `crew`, the arcs in `ties`, the dice in `Live`.
//! This file turns a key into a call and a call into a world edit, and says what happened.
//!
//! **World edits.** Every change to the shared world goes through [`Session::edit`], which runs
//! the same function on this session's copy of the world (so the screen answers at once) and
//! asks the board to run it on the stored world, which may be newer: the world is the truth, the
//! copy is a courtesy. A function that fails on the stored world leaves its receipt there (a
//! refusal that gives the stake back), so a lost race costs time, never marks.

use std::sync::Arc;

use russh_bbs_door::{Ctx, DoorKey, DoorStyle, Flow};

use crate::content::Content;
use crate::crew::{self, Live, Op, OpState};
use crate::player::stat;
use crate::run::{Act, Refused, Status};
use crate::save;
use crate::social::{BOUNTY_STAKE, NOTE_MAX, PROTECT_DAYS, Posted, Why, same};
use crate::story::ARC_TMPL;
use crate::world::{Contract, World};

use super::{Active, Line, Report, Session, View, push_wrapped};

/// What a line of text input is for.
#[derive(Clone, PartialEq, Eq)]
pub(super) enum InputKind {
    /// The name of a new crew.
    CrewName,
    /// The handle of someone to invite.
    Invite,
    /// The note on a bounty on `target`, with a stake.
    Note { target: String, stake: u32 },
}

/// A line being typed.
#[derive(Clone, PartialEq, Eq)]
pub(super) struct Input {
    pub kind: InputKind,
    pub buf: String,
}

/// The crew a run belongs to, as the run screen sees it.
pub(super) struct CrewLink {
    /// This session's copy of the crew run.
    pub op: Op,
    /// The crew's runs, rebuilt from the log and kept up with this member's own moves.
    pub live: Live,
    /// This member's place in the run.
    pub me: usize,
}

/// Who is where on the map, for the other members of a crew run: (node, initial, handle).
pub(super) fn mates(link: &CrewLink) -> Vec<(u8, char, String)> {
    link.op
        .members
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != link.me)
        .filter_map(|(i, m)| {
            let run = link.live.runs.get(i)?;
            (run.status == Status::Active).then(|| {
                (
                    run.at,
                    m.handle.chars().next().unwrap_or('?').to_ascii_uppercase(),
                    m.handle.clone(),
                )
            })
        })
        .collect()
}

fn band(heat: u8) -> &'static str {
    match heat {
        0..=29 => "cool",
        30..=59 => "warm",
        _ => "hot",
    }
}

impl Session {
    /// Apply `f` to this session's copy of the world and ask the board to apply it to the stored
    /// one. Returns what the copy said.
    pub(super) fn edit<F>(&mut self, ctx: &mut Ctx<'_>, f: F) -> Result<(), String>
    where
        F: Fn(&mut World, &Content) -> Result<(), String> + Send + Sync + 'static,
    {
        let content = Arc::clone(&self.content);
        let season = self.season;
        let f = Arc::new(f);
        let (c2, f2) = (Arc::clone(&content), Arc::clone(&f));
        ctx.edit_world(move |old| {
            let mut world = old.and_then(save::decode_world).unwrap_or_default();
            world.sync_season(&c2, season);
            // A refusal on the stored world still changes it (the receipt), so write either way.
            let _ = f2(&mut world, &c2);
            let bytes = save::encode_world(&world);
            (old != Some(&bytes[..])).then_some(bytes)
        });
        f(&mut self.world, &content)
    }

    /// Repost the hideout when anything a scout could see changed.
    pub(super) fn repost(&mut self, ctx: &mut Ctx<'_>) {
        let today = ctx.today().as_raw();
        let mut now = self.p.posted(today);
        let same_as_before = self.posted_sig.as_ref().is_some_and(|b| {
            let mut b = b.clone();
            b.seen = now.seen;
            b == now
        });
        if same_as_before {
            return;
        }
        self.posted_sig = Some(now.clone());
        now.peace_until = 0;
        let to_post = now.clone();
        let _ = self.edit(ctx, move |w, _| {
            w.social.post(to_post.clone());
            Ok(())
        });
        if !now.pvp {
            // Off the board is off the board: not even a stale post of the hideout.
            let _ = now;
        }
    }

    /// The rivals a player can see: everyone on the board with the rivals board on, but not
    /// themselves, best known first.
    pub(super) fn rivals(&self) -> Vec<Posted> {
        let mut out: Vec<Posted> = self
            .world
            .social
            .posted
            .iter()
            .filter(|p| p.pvp && !same(&p.handle, &self.p.name))
            .cloned()
            .collect();
        out.sort_by(|a, b| {
            b.tier
                .cmp(&a.tier)
                .then(b.rep.cmp(&a.rep))
                .then_with(|| a.handle.cmp(&b.handle))
        });
        out
    }

    fn why_not(&self, target: &str, today: i64) -> Option<&'static str> {
        self.world
            .social
            .check_hack(&self.p.name, target, today)
            .err()
            .map(Why::text)
    }

    // ---- text ----

    #[allow(clippy::needless_late_init)]
    pub(super) fn social_lines(&self, rows: usize) -> Option<Vec<Line>> {
        use DoorStyle::{Accent, Bad, Dim, Good, Plain, Strong};
        let c = &*self.content;
        let today = self.p.day;
        let mut body: Vec<Line> = Vec::new();
        let hint: &str;
        match &self.view {
            View::Rivals => {
                body.push((
                    format!(
                        "RIVALS  {}",
                        if self.p.ties.pvp {
                            "you are on the board"
                        } else {
                            "you are OFF the board"
                        }
                    ),
                    Accent,
                ));
                let list = self.rivals();
                if list.is_empty() {
                    body.push(("Nobody else is on the board yet.".into(), Dim));
                }
                for (i, r) in list.iter().enumerate().take(8) {
                    let bounty = self
                        .world
                        .social
                        .bounties
                        .iter()
                        .filter(|b| same(&b.target, &r.handle))
                        .map(|b| b.reward)
                        .max();
                    let mark = if i == self.sel { '>' } else { ' ' };
                    body.push((
                        format!(
                            "{mark}{:<14} T{} heat {:<4} ICE {} ~{} marks{}",
                            r.handle,
                            r.tier,
                            band(r.heat),
                            r.shield,
                            u32::from(r.cash) * 10,
                            bounty.map_or(String::new(), |b| format!("  BOUNTY {b}"))
                        ),
                        if i == self.sel { Strong } else { Plain },
                    ));
                }
                if let Some(r) = list.get(self.sel) {
                    let line = self.why_not(&r.handle, today).map_or_else(
                        || {
                            (
                                format!(
                                    "Hack {}: {} cycles, runs tonight, you hear tomorrow.",
                                    r.handle,
                                    crate::social::HACK_CYCLES
                                ),
                                Good,
                            )
                        },
                        |why| (format!("Not now: {why}."), Bad),
                    );
                    push_wrapped(&mut body, &line.0, line.1);
                }
                push_wrapped(
                    &mut body,
                    &format!(
                        "New characters are safe for {PROTECT_DAYS} days. You can hack twice a day; a hideout takes two. A honeypot may catch you. Nothing here is private: the board can read every post."
                    ),
                    Dim,
                );
                self.push_msgs(&mut body);
                hint = "[Up/Down] pick  [h] hack  [b] bounty 100  [B] bounty 300  [t] rivals on/off  [Esc]";
            }
            View::Input(input) => {
                let what = match &input.kind {
                    InputKind::CrewName => "Name your crew (up to 20 letters):".to_owned(),
                    InputKind::Invite => "Invite which handle?".to_owned(),
                    InputKind::Note { target, stake } => format!(
                        "A note on the {stake}-mark bounty on {target} (up to {NOTE_MAX}; empty for none):"
                    ),
                };
                body.push((what, Accent));
                body.push((format!("> {}_", input.buf), Plain));
                self.push_msgs(&mut body);
                hint = "[Enter] done   [Esc] cancel";
            }
            View::Crew => {
                body.push(("CREW".into(), Accent));
                let me = &self.p.name;
                let s = &self.world.social;
                match s.crew_of(me) {
                    None => {
                        body.push(("You are not in a crew.".into(), Dim));
                        let inv = s.invites_for(me);
                        for (i, cr) in inv.iter().enumerate().take(5) {
                            body.push((
                                format!(
                                    "[{}] join {} (led by {}, {} members)",
                                    i + 1,
                                    cr.name,
                                    cr.leader,
                                    cr.members.len()
                                ),
                                Plain,
                            ));
                        }
                        if inv.is_empty() {
                            body.push(("No invitations. Found a crew, or be asked.".into(), Dim));
                        }
                    }
                    Some(cr) => {
                        body.push((
                            format!("{}  led by {}  {} job(s) done", cr.name, cr.leader, cr.jobs),
                            Strong,
                        ));
                        body.push((format!("Members: {}", cr.members.join(", ")), Plain));
                        if !cr.invited.is_empty() {
                            body.push((format!("Invited: {}", cr.invited.join(", ")), Dim));
                        }
                        match s
                            .ops
                            .iter()
                            .find(|o| o.crew == cr.id && o.state != OpState::Done)
                            .or_else(|| {
                                s.ops.iter().find(|o| {
                                    o.crew == cr.id
                                        && o.index_of(me).is_some()
                                        && !o.settled.iter().any(|x| same(x, me))
                                })
                            }) {
                            None => body.push(("No crew run is open.".into(), Dim)),
                            Some(op) => {
                                let job = c.job(&op.contract);
                                let corp =
                                    c.corp(op.contract.corp).map_or("?", |x| x.name.as_str());
                                body.push((
                                    format!(
                                        "Run: {} at {corp} (tier {}), {}",
                                        job.as_ref().map_or("?", |j| j.title.as_str()),
                                        op.contract.tier,
                                        match op.state {
                                            OpState::Lobby => "gathering",
                                            OpState::Live => "under way",
                                            OpState::Done => "over",
                                        }
                                    ),
                                    Accent,
                                ));
                                body.push((
                                    format!(
                                        "In it: {}  ({} actions so far)",
                                        op.members
                                            .iter()
                                            .map(|m| m.handle.as_str())
                                            .collect::<Vec<_>>()
                                            .join(", "),
                                        op.log.len()
                                    ),
                                    Plain,
                                ));
                            }
                        }
                    }
                }
                if s.finale.is_none() {
                    body.push((
                        "The Null Vault stands: a heist built for a crew of three or more (rep 40).".into(),
                        Dim,
                    ));
                } else if let Some(name) = &s.finale {
                    body.push((format!("The Null Vault was cracked by {name}."), Good));
                }
                self.push_msgs(&mut body);
                hint = "[n] found  [1-5] accept  [i] invite  [o] open a run  [j] join  [s] start  [e] enter  [c] collect  [f] finale  [l] leave  [m] invites on/off  [Esc]";
            }
            View::Arcs => {
                body.push(("ARCS: each fixer's story".into(), Accent));
                for arc in &c.story.arc {
                    let st = self
                        .p
                        .ties
                        .arcs
                        .get(usize::from(arc.num))
                        .copied()
                        .unwrap_or_default();
                    let mark = if usize::from(arc.num) == self.sel {
                        '>'
                    } else {
                        ' '
                    };
                    body.push((
                        format!(
                            "{mark}{:<20} {:<12} step {} of 7: {}",
                            arc.title,
                            c.fixer(arc.fixer).map_or("?", |f| f.name.as_str()),
                            st.step.max(1),
                            self.p.arc_status(c, arc.num, today)
                        ),
                        if usize::from(arc.num) == self.sel {
                            Strong
                        } else {
                            Plain
                        },
                    ));
                }
                if let Some(arc) = c.arc(u8::try_from(self.sel).unwrap_or(0)) {
                    push_wrapped(&mut body, &arc.blurb, Dim);
                }
                self.push_msgs(&mut body);
                hint = "[Up/Down] pick  [Enter] open its job or its choice  [Esc] back";
            }
            View::Scene(arc) => {
                let st = self
                    .p
                    .ties
                    .arcs
                    .get(usize::from(*arc))
                    .copied()
                    .unwrap_or_default();
                let step = c.step(*arc, st.step);
                body.push((
                    format!(
                        "{}: {}",
                        c.arc(*arc).map_or("?", |a| a.title.as_str()),
                        step.map_or("?", |s| s.title.as_str())
                    ),
                    Accent,
                ));
                if let Some(step) = step {
                    push_wrapped(&mut body, &step.done, Plain);
                }
                for (i, (choice, why)) in self.p.arc_choices(c, *arc).iter().enumerate() {
                    body.push((
                        format!(
                            "[{}] {}{}",
                            i + 1,
                            choice.label,
                            why.as_ref().map_or(String::new(), |w| format!("   ({w})"))
                        ),
                        if why.is_some() { Dim } else { Plain },
                    ));
                }
                self.push_msgs(&mut body);
                hint = "[1-3] choose   [Esc] later";
            }
            View::Street(tab) => {
                let s = &self.world.social;
                body.push((
                    format!(
                        "THE STREET  [1] word  [2] bounties  [3] crews  [4] hall    season {}",
                        self.season
                    ),
                    Accent,
                ));
                match tab {
                    0 => {
                        if s.deeds.is_empty() {
                            body.push((
                                "Quiet. Nothing has happened that anyone is saying.".into(),
                                Dim,
                            ));
                        }
                        for d in s.deeds.iter().rev().take(8) {
                            body.push((
                                format!("day {}: {}", d.day, c.say(d.line, &d.who, i64::from(d.n))),
                                Plain,
                            ));
                        }
                        let t = &s.tallies;
                        body.push((
                            format!(
                                "This season: {} jobs ({} deep), {} wardens down, {} crew jobs, {} hacks in, {} caught, {} bounties paid, {} burns.",
                                t.jobs, t.deep_breaches, t.wardens, t.crew_jobs, t.hacks_in, t.hacks_caught, t.bounties_paid, t.burns
                            ),
                            Dim,
                        ));
                        let ending = self.season_forecast();
                        body.push((format!("If the season ended now: {ending}."), Dim));
                    }
                    1 => {
                        if s.bounties.is_empty() {
                            body.push(("No prices on any heads.".into(), Dim));
                        }
                        for b in s.bounties.iter().take(8) {
                            body.push((
                                format!(
                                    "{} T{} {} marks via {} until day {}{}",
                                    b.target,
                                    b.tier,
                                    b.reward,
                                    c.fixer(b.fixer).map_or("?", |f| f.name.as_str()),
                                    b.expires,
                                    if b.poster.is_empty() {
                                        String::new()
                                    } else {
                                        format!("  from {}", b.poster)
                                    }
                                ),
                                Plain,
                            ));
                            if !b.note.is_empty() {
                                body.push((format!("    \"{}\"", b.note), Dim));
                            }
                        }
                        push_wrapped(
                            &mut body,
                            "A bounty is paid to a hack that gets in, from someone at the target's tier or below. It fries a rig part. The target is told.",
                            Dim,
                        );
                    }
                    2 => {
                        if s.crews.is_empty() {
                            body.push(("No crews yet.".into(), Dim));
                        }
                        for cr in s.crews.iter().take(8) {
                            body.push((
                                format!(
                                    "{:<20} {} members, {} jobs, led by {}",
                                    cr.name,
                                    cr.members.len(),
                                    cr.jobs,
                                    cr.leader
                                ),
                                Plain,
                            ));
                        }
                    }
                    _ => {
                        if s.hall.is_empty() {
                            body.push(("No season has closed yet.".into(), Dim));
                        }
                        for h in s.hall.iter().rev().take(4) {
                            let title = c
                                .seasons
                                .season_ending
                                .iter()
                                .find(|e| e.num == h.ending)
                                .map_or("?", |e| e.title.as_str());
                            body.push((
                                format!(
                                    "Season {}: {title}. Top: {}",
                                    h.season,
                                    h.top
                                        .iter()
                                        .map(|(n, r)| format!("{n} {r}"))
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                ),
                                Plain,
                            ));
                        }
                        for l in self.p.ties.legend.iter().rev().take(3) {
                            let title = c
                                .seasons
                                .epilogue
                                .iter()
                                .find(|e| e.num == l.ending)
                                .map_or("?", |e| e.title.as_str());
                            body.push((
                                format!("Yours, season {}: {title} (rep {})", l.season, l.rep),
                                Strong,
                            ));
                        }
                    }
                }
                self.push_msgs(&mut body);
                hint = "[1-4] page   [Esc] back";
            }
            _ => return None,
        }
        let hint_lines = wrap_hint(hint);
        // On a drawn screen the key hints sit on the last rows; the look-around is not padded.
        if rows < 200 {
            while body.len() + hint_lines.len() < rows {
                body.push((String::new(), DoorStyle::Plain));
            }
        }
        Some(super::fit(body, hint_lines, rows))
    }

    /// The ending the season would get if it closed on today's tallies.
    pub(super) fn season_forecast(&self) -> String {
        self.content
            .seasons
            .season_ending
            .iter()
            .find(|e| {
                crate::cond::Cond::parse(&e.rule, &[])
                    .is_ok_and(|c| c.eval(None, &self.world.social.tallies))
            })
            .map_or_else(|| "?".to_owned(), |e| e.title.clone())
    }

    // ---- keys ----

    /// Handle a key on a social screen. `None` if the key is not for these screens.
    pub(super) fn social_key(&mut self, key: DoorKey, ctx: &mut Ctx<'_>) -> Option<Flow> {
        let today = ctx.today().as_raw();
        match (&self.view, key) {
            (View::Input(_), _) => self.input_key(key, ctx),
            (View::Home, DoorKey::Char(ch)) => {
                let view = match ch.to_ascii_lowercase() {
                    'v' => View::Rivals,
                    'k' => View::Crew,
                    'a' => View::Arcs,
                    'w' => View::Street(0),
                    _ => return None,
                };
                self.msgs.clear();
                self.sel = 0;
                self.view = view;
            }
            (View::Hideout, DoorKey::Char('k' | 'K')) => {
                let r = self.p.buy_honeypot();
                self.say(r);
            }
            (View::Rivals | View::Arcs, DoorKey::Up) => self.sel = self.sel.saturating_sub(1),
            (View::Rivals, DoorKey::Down) => {
                self.sel = (self.sel + 1).min(self.rivals().len().min(8).saturating_sub(1));
            }
            (View::Rivals, DoorKey::Char('t' | 'T')) => {
                let r = self.p.set_pvp(!self.p.ties.pvp, today);
                let on = self.p.ties.pvp;
                let handle = self.p.name.clone();
                let post = self.p.posted(today);
                let _ = self.edit(ctx, move |w, _| {
                    w.social.post(post.clone());
                    let _ = (&handle, on);
                    Ok(())
                });
                self.say(r);
            }
            (View::Rivals, DoorKey::Char('h' | 'H') | DoorKey::Enter) => {
                let Some(target) = self.rivals().get(self.sel).map(|r| r.handle.clone()) else {
                    self.say(Err("Nobody to hack.".into()));
                    return Some(Flow::Stay);
                };
                if let Some(why) = self.why_not(&target, today) {
                    self.say(Err(format!("Not now: {why}.")));
                    return Some(Flow::Stay);
                }
                match self.p.begin_hack() {
                    Ok(nonce) => {
                        let me = self.p.name.clone();
                        let t2 = target.clone();
                        let r = self.edit(ctx, move |w, _| {
                            w.social
                                .file_hack(&me, &t2, today, nonce)
                                .map_err(|e| e.text().to_owned())
                        });
                        match r {
                            Ok(()) => self.say(Ok(format!(
                                "Hack on {target} filed ({} cycles). It runs tonight; you hear at your next visit.",
                                crate::social::HACK_CYCLES
                            ))),
                            Err(e) => self.say(Err(format!("Refused: {e}. The cycles come back."))),
                        }
                    }
                    Err(e) => self.say(Err(e)),
                }
            }
            (View::Rivals, DoorKey::Char(ch @ ('b' | 'B'))) => {
                let Some(target) = self.rivals().get(self.sel).map(|r| r.handle.clone()) else {
                    self.say(Err("Nobody to name.".into()));
                    return Some(Flow::Stay);
                };
                let stake = if ch == 'B' { 300 } else { BOUNTY_STAKE.0 };
                if let Err(why) = self.world.social.check_bounty(&self.p.name, &target, stake) {
                    self.say(Err(format!("Not now: {}.", why.text())));
                    return Some(Flow::Stay);
                }
                self.msgs.clear();
                self.view = View::Input(Box::new(Input {
                    kind: InputKind::Note { target, stake },
                    buf: String::new(),
                }));
            }
            (View::Crew, _) => return self.crew_key(key, ctx),
            (View::Arcs, DoorKey::Down) => {
                self.sel = (self.sel + 1).min(self.content.story.arc.len().saturating_sub(1));
            }
            (View::Arcs, DoorKey::Enter) => {
                let arc = u8::try_from(self.sel).unwrap_or(0);
                let st = self
                    .p
                    .ties
                    .arcs
                    .get(usize::from(arc))
                    .copied()
                    .unwrap_or_default();
                if st.awaiting {
                    self.msgs.clear();
                    self.view = View::Scene(arc);
                } else if let Some(contract) = self.p.arc_contract(&self.content, arc, today) {
                    self.msgs.clear();
                    self.view = View::Brief(contract);
                } else {
                    let why = self.p.arc_status(&self.content, arc, today);
                    self.say(Err(format!("No job today: {why}.")));
                }
            }
            (View::Scene(arc), DoorKey::Char(ch)) => {
                let arc = *arc;
                if let Some(n) = ch.to_digit(10).filter(|n| *n >= 1) {
                    match self.p.arc_choose(&self.content, arc, n as usize - 1) {
                        Ok(text) => {
                            self.say(Ok(text));
                            self.view = View::Arcs;
                        }
                        Err(e) => self.say(Err(e)),
                    }
                }
            }
            (View::Scene(_), DoorKey::Escape) => {
                self.msgs.clear();
                self.view = View::Arcs;
            }
            (View::Street(_), DoorKey::Char(ch)) => {
                if let Some(n) = ch.to_digit(10).filter(|n| (1..=4).contains(n)) {
                    self.view = View::Street(u8::try_from(n - 1).unwrap_or(0));
                    self.msgs.clear();
                }
            }
            (View::Rivals | View::Arcs | View::Street(_), DoorKey::Escape) => {
                self.msgs.clear();
                self.sel = 0;
                self.view = View::Home;
            }
            _ => return None,
        }
        Some(Flow::Stay)
    }

    fn input_key(&mut self, key: DoorKey, ctx: &mut Ctx<'_>) {
        let View::Input(input) = &mut self.view else {
            return;
        };
        match key {
            DoorKey::Char(ch) if input.buf.chars().count() < 80 && !ch.is_control() => {
                input.buf.push(ch);
            }
            DoorKey::Backspace => {
                input.buf.pop();
            }
            DoorKey::Escape => {
                self.msgs.clear();
                self.view = match input.kind {
                    InputKind::Note { .. } => View::Rivals,
                    _ => View::Crew,
                };
            }
            DoorKey::Enter => {
                let input = (**input).clone();
                self.submit(input, ctx);
            }
            _ => {}
        }
    }

    fn submit(&mut self, input: Input, ctx: &mut Ctx<'_>) {
        let today = ctx.today().as_raw();
        let me = self.p.name.clone();
        match input.kind {
            InputKind::CrewName => {
                self.view = View::Crew;
                let Ok(name) = ctx.player_name(&input.buf) else {
                    self.say(Err("That is not a name the board allows.".into()));
                    return;
                };
                let name: String = name.as_str().chars().take(crew::NAME_MAX).collect();
                let r = self.edit(ctx, move |w, _| {
                    w.social
                        .crew_create(&name, &me, today)
                        .map_err(|e| e.text().to_owned())
                });
                self.say(r.map(|()| "Crew founded.".to_owned()));
            }
            InputKind::Invite => {
                self.view = View::Crew;
                let who = input.buf.trim().to_owned();
                if who.is_empty() {
                    return;
                }
                let r = self.edit(ctx, move |w, _| {
                    w.social
                        .crew_invite(&me, &who, today)
                        .map_err(|e| e.text().to_owned())
                });
                self.say(r.map(|()| {
                    "Invitation sent. They see a line with your handle, nothing more.".to_owned()
                }));
            }
            InputKind::Note { target, stake } => {
                self.view = View::Rivals;
                let note = match ctx.player_text(&input.buf) {
                    Ok(t) => t.as_str().to_owned(),
                    Err(russh_bbs_door::TextRefused::Empty) => String::new(),
                    Err(e) => {
                        self.say(Err(e.to_string()));
                        return;
                    }
                };
                let note: String = note.chars().take(NOTE_MAX).collect();
                match self.p.begin_bounty(stake) {
                    Ok(nonce) => {
                        let t2 = target.clone();
                        let r = self.edit(ctx, move |w, c| {
                            w.social
                                .post_bounty(c, &me, &t2, stake, &note, today, nonce)
                                .map_err(|e| e.text().to_owned())
                        });
                        match r {
                            Ok(()) => self.say(Ok(format!(
                                "A fixer holds {stake} marks and has put a price on {target}. It pays to a hack that gets in."
                            ))),
                            Err(e) => self.say(Err(format!("Refused: {e}. Your stake comes back."))),
                        }
                    }
                    Err(e) => self.say(Err(e)),
                }
            }
        }
    }

    // ---- crews ----

    fn my_op(&self) -> Option<&Op> {
        let me = &self.p.name;
        let crew = self.world.social.crew_of(me)?;
        self.world
            .social
            .ops
            .iter()
            .find(|o| {
                o.crew == crew.id && o.state != OpState::Done && o.index_of(me).is_some()
                    || o.crew == crew.id && o.state == OpState::Lobby
            })
            .or_else(|| {
                self.world.social.ops.iter().find(|o| {
                    o.crew == crew.id
                        && o.state == OpState::Done
                        && o.index_of(me).is_some()
                        && !o.settled.iter().any(|x| same(x, me))
                })
            })
    }

    fn crew_key(&mut self, key: DoorKey, ctx: &mut Ctx<'_>) -> Option<Flow> {
        let today = ctx.today().as_raw();
        let me = self.p.name.clone();
        if key == DoorKey::Escape {
            self.msgs.clear();
            self.sel = 0;
            self.view = View::Home;
            return Some(Flow::Stay);
        }
        let DoorKey::Char(ch) = key else {
            return None;
        };
        let in_crew = self.world.social.crew_of(&me).is_some();
        match ch.to_ascii_lowercase() {
            'n' if !in_crew => {
                self.msgs.clear();
                self.view = View::Input(Box::new(Input {
                    kind: InputKind::CrewName,
                    buf: String::new(),
                }));
            }
            'm' => {
                let on = !self.p.ties.crews_open;
                let r = self.p.set_crews_open(on);
                let post = self.p.posted(today);
                let _ = self.edit(ctx, move |w, _| {
                    w.social.post(post.clone());
                    Ok(())
                });
                self.say(Ok(r));
            }
            d @ '1'..='5' if !in_crew => {
                let n = d.to_digit(10).unwrap_or(1) as usize - 1;
                let Some(id) = self.world.social.invites_for(&me).get(n).map(|c| c.id) else {
                    return Some(Flow::Stay);
                };
                let r = self.edit(ctx, move |w, _| {
                    w.social
                        .crew_accept(&me, id)
                        .map_err(|e| e.text().to_owned())
                });
                self.say(r.map(|()| "You are in the crew.".to_owned()));
            }
            'i' if in_crew => {
                self.msgs.clear();
                self.view = View::Input(Box::new(Input {
                    kind: InputKind::Invite,
                    buf: String::new(),
                }));
            }
            'l' if in_crew => {
                let r = self.edit(ctx, move |w, _| {
                    w.social.crew_leave(&me).map_err(|e| e.text().to_owned())
                });
                self.say(r.map(|()| "You left the crew.".to_owned()));
            }
            'o' if in_crew => {
                self.say(Err("Open a run from a contract: [c] contracts, pick one, then [g] gather the crew.".into()));
            }
            'f' if in_crew => {
                let corp = crew::FINALE_CORP;
                let contract = Contract {
                    id: 0x3FFF_0000 + self.season,
                    tmpl: crew::FINALE_TMPL,
                    corp,
                    fixer: self.content.corp(corp).map_or(0, |c| c.patron),
                    tier: 3,
                    reward: crew::FINALE_REWARD,
                    expires: today,
                };
                self.gather(ctx, contract);
            }
            'j' if in_crew => self.join_run(ctx),
            's' if in_crew => {
                let Some(id) = self.my_op().map(|o| o.id) else {
                    self.say(Err("No run to start.".into()));
                    return Some(Flow::Stay);
                };
                let r = self.edit(ctx, move |w, _| {
                    w.op_start(id, &me, today).map_err(|e| e.text().to_owned())
                });
                self.say(r.map(|()| "The run is live. Everyone in it can jack in now.".to_owned()));
            }
            'e' if in_crew => self.enter_run(),
            'c' if in_crew => self.collect(ctx),
            _ => return None,
        }
        Some(Flow::Stay)
    }

    /// Open a crew run on `contract` from this character's side: commit cycles, join, and tell
    /// the world.
    pub(super) fn gather(&mut self, ctx: &mut Ctx<'_>, contract: Contract) {
        let today = ctx.today().as_raw();
        let me = self.p.name.clone();
        if self.world.social.crew_of(&me).is_none() {
            self.say(Err("Found or join a crew first: [k] crew.".into()));
            return;
        }
        if self.my_op().is_some_and(|o| o.state != OpState::Done) {
            self.say(Err("Your crew has a run open already.".into()));
            return;
        }
        let id = self.p.fresh_nonce(2);
        match self.p.begin_crew_run(&self.content, &contract, id) {
            Ok(member) => {
                let r = self.edit(ctx, move |w, _| {
                    w.op_open(id, contract, member.clone(), today)
                        .map_err(|e| e.text().to_owned())
                });
                match r {
                    Ok(()) => {
                        self.view = View::Crew;
                        self.say(Ok(
                            "The run is open. Your crew can join from [k]; the leader starts it."
                                .into(),
                        ));
                    }
                    Err(e) => {
                        self.p.drop_stake();
                        self.say(Err(e));
                    }
                }
            }
            Err(e) => self.say(Err(e)),
        }
    }

    fn join_run(&mut self, ctx: &mut Ctx<'_>) {
        let Some(op) = self.my_op().cloned() else {
            self.say(Err("No run to join.".into()));
            return;
        };
        let me = self.p.name.clone();
        if op.index_of(&me).is_some() {
            self.say(Err("You are in this run.".into()));
            return;
        }
        match self.p.begin_crew_run(&self.content, &op.contract, op.id) {
            Ok(member) => {
                let r = self.edit(ctx, move |w, _| {
                    w.op_join(op.id, member.clone())
                        .map_err(|e| e.text().to_owned())
                });
                match r {
                    Ok(()) => self.say(Ok("You are in. The leader starts the run.".into())),
                    Err(e) => {
                        self.p.drop_stake();
                        self.say(Err(e));
                    }
                }
            }
            Err(e) => self.say(Err(e)),
        }
    }

    fn enter_run(&mut self) {
        let me = self.p.name.clone();
        let Some(op) = self.my_op().cloned() else {
            self.say(Err("No run to enter.".into()));
            return;
        };
        let Some(idx) = op.index_of(&me) else {
            self.say(Err("You are not in this run.".into()));
            return;
        };
        if op.state != OpState::Live {
            self.say(Err("The run is not live.".into()));
            return;
        }
        let Some(live) = op.live(&self.content) else {
            self.say(Err("The network will not load.".into()));
            return;
        };
        let run = live.runs[idx].clone();
        if run.status != Status::Active {
            self.say(Err(
                "You are out of this run. Wait for the crew to finish, then collect.".into(),
            ));
            return;
        }
        let feed = super::Feed::new(&run);
        let contract = op.contract;
        self.msgs.clear();
        self.view = View::Run(Box::new(Active {
            run,
            contract,
            aim: None,
            feed,
            crew: Some(CrewLink { op, live, me: idx }),
        }));
    }

    /// Collect a finished crew run: this character's share, and the crew's split on the debrief.
    fn collect(&mut self, ctx: &mut Ctx<'_>) {
        let me = self.p.name.clone();
        let Some(op) = self.my_op().cloned() else {
            self.say(Err("Nothing to collect.".into()));
            return;
        };
        if op.state != OpState::Done || self.p.ties.stake.map(|s| s.op) != Some(op.id) {
            self.say(Err("Your run is not over yet.".into()));
            return;
        }
        let Some(live) = op.live(&self.content) else {
            self.say(Err("The network will not load.".into()));
            return;
        };
        let content = Arc::clone(&self.content);
        let mut notes = self.p.settle_crew(&content, &op, &live, ctx.rng());
        let id = op.id;
        let who = me.clone();
        let _ = self.edit(ctx, move |w, _| {
            w.op_settled(id, &who);
            Ok(())
        });
        let idx = op.index_of(&me).unwrap_or(0);
        let result = live
            .result_of(idx)
            .unwrap_or_else(|| live.runs[idx].result());
        let total = {
            let base = if op.is_finale() {
                crew::FINALE_REWARD
            } else {
                op.contract.reward
            };
            base * crew::PAY_PCT[op.members.len().clamp(1, 5) - 1] / 100
        };
        let shares = live.shares(total);
        let points = live.points();
        let split: Vec<(String, u32, u32, Status)> = op
            .members
            .iter()
            .enumerate()
            .map(|(i, m)| {
                (
                    m.handle.clone(),
                    shares.get(i).copied().unwrap_or(0),
                    points.get(i).copied().unwrap_or(0),
                    live.runs[i].status,
                )
            })
            .collect();
        notes.extend(live.chatter.iter().rev().take(3).rev().cloned());
        let mut with_split = vec!["The crew's split:".to_owned()];
        with_split.extend(split_lines(&split).into_iter().map(|(t, _)| t));
        with_split.push(String::new());
        with_split.extend(notes);
        let notes = with_split;
        self.view = View::Debrief(Box::new(Report {
            notes,
            result,
            contract: op.contract,
        }));
    }

    /// One action in a crew run: the member's own run, the crew's shared state, and the edit
    /// that tells the world. Nothing changes on a refusal.
    pub(super) fn crew_act(
        active: &mut Active,
        content: &Arc<Content>,
        act: Act,
        ctx: &mut Ctx<'_>,
        season: u32,
        handle: &str,
    ) -> Result<(), Refused> {
        let Some(link) = active.crew.as_mut() else {
            return Err(Refused::Over);
        };
        let today = ctx.today().as_raw();
        let nth = link.op.log.len();
        link.live.apply(content, &link.op, link.me, act, nth)?;
        link.op.log.push(crew::pack(link.me, act));
        link.op.last = today;
        if link.live.over() {
            link.op.state = OpState::Done;
        }
        active.run = link.live.runs[link.me].clone();
        let (id, who, c2) = (link.op.id, handle.to_owned(), Arc::clone(content));
        ctx.edit_world(move |old| {
            let mut world = old.and_then(save::decode_world).unwrap_or_default();
            world.sync_season(&c2, season);
            let before = world.clone();
            if world.op_act(&c2, id, &who, act, nth, today).is_err() || world == before {
                return None;
            }
            Some(save::encode_world(&world))
        });
        Ok(())
    }
}

fn wrap_hint(hint: &str) -> Vec<Line> {
    let mut out = Vec::new();
    push_wrapped(&mut out, hint, DoorStyle::Strong);
    out
}

/// Whether a contract is a story step.
#[must_use]
pub(super) const fn is_arc(c: &Contract) -> bool {
    c.tmpl == ARC_TMPL
}

/// The lines of the debrief's crew split.
pub(super) fn split_lines(split: &[(String, u32, u32, Status)]) -> Vec<Line> {
    let mut out = Vec::new();
    for (name, share, points, status) in split {
        out.push((
            format!(
                "{name:<16} {:<7} {points:>3} pts  {share:>4} marks",
                match status {
                    Status::Burned => "burned",
                    Status::Out => "out",
                    Status::Active => "in",
                }
            ),
            if *status == Status::Burned {
                DoorStyle::Bad
            } else {
                DoorStyle::Plain
            },
        ));
    }
    out
}

/// Which of the things a stat tracks a screen wants to show.
#[must_use]
pub(super) fn stat_line(stats: &[u16; crate::player::STATS]) -> String {
    format!(
        "hacks {} (in {}), caught {}, bounties {}, crew runs {} (jobs {}), wardens {}, finale {}",
        stats[stat::HACKS],
        stats[stat::IN],
        stats[stat::CAUGHT],
        stats[stat::BOUNTIES],
        stats[stat::CREW_RUNS],
        stats[stat::CREW_JOBS],
        stats[stat::WARDENS],
        stats[stat::FINALE]
    )
}
