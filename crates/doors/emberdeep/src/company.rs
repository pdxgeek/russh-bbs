//! What a hero does about other heroes (M4-E4): invite, join and leave a company, call someone
//! out in the duel ring, collect the notes the world holds for them, write an epitaph, make a
//! choice in a questline and give the Furnace's verdict. The world-side ledger and its caps are in
//! [`crate::social`]; these are the hero's half, checked against the world as the hero last saw
//! it and applied to their own save only.

use crate::content::Content;
use crate::game::{Event, Game, Place, Refused, Tone, say};
use crate::hero::derive;
use crate::save::SEEN_MAX;
use crate::social::{
    self, BOUTS_PER_DAY, Bout, CALLS_PER_TARGET, DUEL_LEVEL_GAP, EPITAPH_MAX, Epitaph, INVITE_DAYS,
    INVITES_PER_HERO, Invite, Member, NoteKind, PARTY_MAX, SocialChange, WAGERS,
};
use crate::world::Change;

/// Marks, in `Today::spent`, that the company's spoils in a den were taken (not a room option).
const SPOILS_MARK: u8 = 250;

/// Something a hero does about the others.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SocialAct {
    /// Invite a hero to a company.
    Invite(String),
    /// Accept an invitation from a hero.
    Accept(String),
    /// Leave the company.
    Leave,
    /// Open or close the door to invitations.
    Open(bool),
    /// Step into or out of the duel ring.
    Ring(bool),
    /// Call a hero of the ring out.
    Challenge { target: String, wager: u32 },
    /// Make a questline's choice.
    Choose { quest: String, choice: String },
    /// Give the verdict on the Cold Furnace.
    Verdict(String),
    /// Set the epitaph (already sanitized by `Ctx::player_text`).
    Epitaph(String),
}

impl Game {
    fn me(&self) -> &str {
        &self.hero.name
    }

    pub(crate) fn say_social(&mut self, change: SocialChange) {
        self.emit(Change::Social(change));
    }

    /// What the town knows of this hero for the roster and the ring.
    pub(crate) fn member_card(&self, content: &Content) -> Member {
        let d = derive(&self.hero, content);
        let damage = content.weapon(&self.hero.weapon).map_or(1, |w| {
            i32::try_from(u32::midpoint(w.damage[0], w.damage[1])).unwrap_or(1)
                + ((d.attr(w.attr) - 8) / 2).max(0)
                + d.damage
        });
        Member {
            name: self.hero.name.clone(),
            level: self.hero.level,
            calling: self.hero.calling.clone(),
            day: self.day,
            open: self.hero.social.open,
            ring: self.hero.social.ring,
            // Ring rules: the ring evens the field, so hit points and armour depend on level
            // alone, damage is pulled halfway to the level's norm, and what is left to tell
            // heroes apart is crit, evasion and the blow. No calling owns the ladder (see the balance notes).
            ghost: social::Ghost {
                hp: 40 + 4 * i32::from(self.hero.level),
                armor: 2 + i32::from(self.hero.level) / 4,
                damage: ((damage + 8 + i32::from(self.hero.level)) / 2).max(1),
                crit: d.crit,
                evade: d.evade,
            },
        }
    }

    /// Tell the town this hero is here (on opening the door, and when a setting changes).
    pub(crate) fn sync_roster(&mut self, content: &Content) {
        let card = self.member_card(content);
        self.say_social(SocialChange::Roster(card));
    }

    /// Whether a pack was dealt with in `room` today by this hero or a companion.
    pub(crate) fn pack_cleared(&self, depth: u8, room: u8) -> bool {
        self.today.is_cleared(depth, room)
            || self
                .world
                .commons
                .party_cleared(self.me(), self.day, depth, room)
    }

    /// A pack a companion dealt with today leaves two thirds of its spoils to everyone in the
    /// company who walks into its den (once each, per day), so riding together is never poorer than
    /// riding alone.
    pub(crate) fn company_spoils(
        &mut self,
        content: &Content,
        level: &crate::dungeon::Level,
        room: u8,
        ev: &mut Vec<Event>,
    ) {
        let depth = level.depth;
        let marker = (depth, room, SPOILS_MARK);
        if level
            .room(room)
            .is_none_or(|r| r.kind != crate::content::RoomKind::Lair)
            || self.today.is_cleared(depth, room)
            || !self
                .world
                .commons
                .party_cleared(&self.hero.name, self.day, depth, room)
            || self.today.spent.contains(&marker)
        {
            return;
        }
        self.today.spent.push(marker);
        let pack = crate::dungeon::pack_at(content, level, self.season, self.epoch, room, self.day);
        let two_thirds = |sum: u32| (sum * 2).div_ceil(3);
        let xp = two_thirds(pack.iter().map(|m| m.xp).sum());
        let coin = two_thirds(
            pack.iter()
                .map(|m| u32::midpoint(m.coin[0], m.coin[1]))
                .sum(),
        );
        let salvage = two_thirds(
            pack.iter()
                .map(|m| u32::midpoint(m.salvage[0], m.salvage[1]))
                .sum(),
        );
        self.hero.coin = self.hero.coin.saturating_add(coin);
        self.hero.salvage = self.hero.salvage.saturating_add(salvage);
        say(
            ev,
            Tone::Good,
            format!(
                "Your company dealt with the pack here earlier. You take two thirds of the spoils: +{xp} xp, +{coin} coin, +{salvage} salvage."
            ),
        );
        self.gain_xp(xp, ev);
    }

    /// Tell the company a pack is dealt with (every companion walks through today).
    pub(crate) fn share_clear(&mut self, depth: u8, room: u8) {
        let who = self.hero.name.clone();
        let day = self.day;
        self.say_social(SocialChange::PartyClear {
            who,
            day,
            depth,
            room,
        });
    }

    /// Collect what the world holds for this hero. Each note is paid once: its id is kept in the
    /// save, whether or not the world forgets it.
    pub(crate) fn collect_notes(&mut self, content: &Content, ev: &mut Vec<Event>) {
        let me = self.hero.name.clone();
        let notes: Vec<_> = self
            .world
            .commons
            .notes
            .iter()
            .filter(|n| n.to == me)
            .cloned()
            .collect();
        for n in notes {
            if !self.hero.social.seen.contains(&n.id) {
                match n.kind {
                    NoteKind::Thanks => {
                        self.hero.coin = self.hero.coin.saturating_add(n.amount);
                        say(
                            ev,
                            Tone::Good,
                            format!(
                                "{} sends their thanks for carrying them home: +{} coin.",
                                n.from, n.amount
                            ),
                        );
                    }
                    NoteKind::Bruise => {
                        self.hero.inflict(content, "ring-bruise");
                        say(
                            ev,
                            Tone::Bad,
                            format!(
                                "{} called you out in the ring and won. You are bruised.",
                                n.from
                            ),
                        );
                    }
                }
                self.hero.social.seen.push(n.id.clone());
                while self.hero.social.seen.len() > SEEN_MAX {
                    self.hero.social.seen.remove(0);
                }
            }
            self.say_social(SocialChange::Collect {
                id: n.id,
                to: me.clone(),
            });
        }
    }

    pub(crate) fn social_act(
        &mut self,
        content: &Content,
        act: SocialAct,
        ev: &mut Vec<Event>,
    ) -> Result<(), Refused> {
        match act {
            SocialAct::Verdict(id) => return self.give_verdict(&id, ev),
            SocialAct::Choose { quest, choice } => {
                return self.choose(content, &quest, &choice, ev);
            }
            _ => {}
        }
        if !matches!(self.place, Place::Town) {
            return Err(Refused::new("That is for the town."));
        }
        match act {
            SocialAct::Invite(name) => self.invite(&name, ev),
            SocialAct::Accept(from) => self.accept(&from, ev),
            SocialAct::Leave => {
                if self.world.commons.party_of(self.me()).is_none() {
                    return Err(Refused::new("You are in no company."));
                }
                let me = self.hero.name.clone();
                self.say_social(SocialChange::Leave(me));
                say(ev, Tone::Dim, "You leave the company.");
                Ok(())
            }
            SocialAct::Open(on) => {
                self.hero.social.open = on;
                self.sync_roster(content);
                say(
                    ev,
                    Tone::Dim,
                    if on {
                        "The door is open to invitations."
                    } else {
                        "No one can invite you now."
                    },
                );
                Ok(())
            }
            SocialAct::Ring(on) => {
                self.hero.social.ring = on;
                self.sync_roster(content);
                say(
                    ev,
                    Tone::Dim,
                    if on {
                        "You step into the ring. Others may call you out, and you may call them."
                    } else {
                        "You step out of the ring. No one can call you out."
                    },
                );
                Ok(())
            }
            SocialAct::Challenge { target, wager } => self.challenge(content, &target, wager, ev),
            SocialAct::Epitaph(text) => {
                self.hero.social.epitaph = text.chars().take(EPITAPH_MAX).collect();
                say(
                    ev,
                    Tone::Dim,
                    "Your epitaph is set. It is read only if you are not found.",
                );
                Ok(())
            }
            SocialAct::Verdict(_) | SocialAct::Choose { .. } => Ok(()),
        }
    }

    fn invite(&mut self, name: &str, ev: &mut Vec<Event>) -> Result<(), Refused> {
        let c = &self.world.commons;
        let me = self.me().to_owned();
        let target = c
            .member(name)
            .ok_or_else(|| Refused::new("No one of that name is known here."))?;
        if name == me {
            return Err(Refused::new("You are already in your own company."));
        }
        if !target.open {
            return Err(Refused::new("They are not taking invitations."));
        }
        if c.party_of(name).is_some() {
            return Err(Refused::new("They already ride with a company."));
        }
        if c.party_of(&me)
            .is_some_and(|p| p.members.len() >= PARTY_MAX)
        {
            return Err(Refused::new("Your company is full."));
        }
        if c.invites_from(&me) >= INVITES_PER_HERO {
            return Err(Refused::new("You have invitations waiting already."));
        }
        if c.invites.iter().any(|i| i.from == me && i.to == name) {
            return Err(Refused::new("They have your invitation already."));
        }
        let day = self.day;
        self.say_social(SocialChange::Invite(Invite {
            from: me,
            to: name.to_owned(),
            day,
        }));
        say(
            ev,
            Tone::Good,
            format!("You send {name} an invitation. It stands for {INVITE_DAYS} days."),
        );
        Ok(())
    }

    fn accept(&mut self, from: &str, ev: &mut Vec<Event>) -> Result<(), Refused> {
        let me = self.me().to_owned();
        let c = &self.world.commons;
        if c.party_of(&me).is_some() {
            return Err(Refused::new("Leave your company before you join another."));
        }
        let fresh = c
            .invites
            .iter()
            .any(|i| i.from == from && i.to == me && self.day - i.day <= INVITE_DAYS);
        if !fresh {
            return Err(Refused::new("That invitation has lapsed."));
        }
        if c.party_of(from)
            .is_some_and(|p| p.members.len() >= PARTY_MAX)
        {
            return Err(Refused::new("That company is full."));
        }
        self.say_social(SocialChange::Join {
            leader: from.to_owned(),
            name: me,
        });
        say(ev, Tone::Good, format!("You ride with {from}'s company."));
        Ok(())
    }

    fn challenge(
        &mut self,
        content: &Content,
        target: &str,
        wager: u32,
        ev: &mut Vec<Event>,
    ) -> Result<(), Refused> {
        let me = self.me().to_owned();
        if !self.hero.social.ring {
            return Err(Refused::new("Step into the ring first. It is your choice."));
        }
        if !WAGERS.contains(&wager) {
            return Err(Refused::new(
                "The ring takes stakes of 0, 5, 20 or 50 coin.",
            ));
        }
        if self.today.bouts >= BOUTS_PER_DAY {
            return Err(Refused::new("The ring will not host you again today."));
        }
        if self.today.sparred.iter().any(|n| n == target) {
            return Err(Refused::new("You have fought them today."));
        }
        let c = &self.world.commons;
        let foe = c
            .member(target)
            .filter(|m| m.ring && self.day - m.day <= 14)
            .ok_or_else(|| Refused::new("They are not in the ring."))?
            .clone();
        if target == me {
            return Err(Refused::new("You cannot call yourself out."));
        }
        if c.companions(&me, target) {
            return Err(Refused::new("You do not duel your own company."));
        }
        if self.hero.level.abs_diff(foe.level) > DUEL_LEVEL_GAP {
            return Err(Refused::new("The ring will not match heroes so far apart."));
        }
        if c.calls_on(target, self.day) >= CALLS_PER_TARGET {
            return Err(Refused::new("They have been called out enough today."));
        }
        if self.hero.coin < wager {
            return Err(Refused::new("You cannot cover that stake."));
        }
        self.spend(1)?;
        self.hero.coin -= wager;
        let mine = self.member_card(content);
        let duel = social::resolve(&mut self.rng, &mine.ghost, &foe.ghost);
        let id = format!("{me}:{target}:{}:{}", self.day, self.today.bouts);
        self.today.bouts += 1;
        self.today.sparred.push(target.to_owned());
        let won = duel.challenger_won;
        if won {
            self.hero.coin = self.hero.coin.saturating_add(wager + wager / 2);
            self.hero.social.wins = self.hero.social.wins.saturating_add(1);
            self.hero.inflict(content, "ring-pride");
            self.gain_xp(8 + 2 * u32::from(self.hero.level), ev);
            say(
                ev,
                Tone::Good,
                format!(
                    "You beat {target} in {} rounds. The ring pays {} coin.",
                    duel.rounds,
                    wager + wager / 2
                ),
            );
        } else {
            self.hero.social.losses = self.hero.social.losses.saturating_add(1);
            self.hero.inflict(content, "ring-bruise");
            self.gain_xp(4, ev);
            say(
                ev,
                Tone::Bad,
                format!(
                    "{target} beats you in {} rounds. Your stake of {wager} coin goes to the town works.",
                    duel.rounds
                ),
            );
        }
        let day = self.day;
        self.say_social(SocialChange::Duel(Bout {
            id,
            challenger: me,
            target: target.to_owned(),
            challenger_won: won,
            wager,
            day,
        }));
        Ok(())
    }

    /// The epitaph the hall prints if this hero is not found.
    pub(crate) fn post_epitaph(&mut self, depth: u8) {
        let text = if self.hero.social.epitaph.is_empty() {
            format!("Lost on level {depth}.")
        } else {
            self.hero.social.epitaph.clone()
        };
        let e = Epitaph {
            name: self.hero.name.clone(),
            text,
            depth,
            day: self.day,
        };
        self.say_social(SocialChange::Epitaph(e));
    }
}
