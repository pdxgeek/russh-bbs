//! Clubs: joining, meetings, ranks and the term-end event.
//!
//! Rank is the lever. Conditions anywhere in the content can ask for `rank:CLUB>=N`, so a club a
//! pupil belongs to opens places, shop items, scheme methods, twists, getaways and conversation
//! choices, and rival clubs will not have a member of the other. Walking out of a club is
//! remembered for the term.

use russh_bbs_door::DoorRng;

use crate::content::ClubSpec;
use crate::game::{Event, Game, Refused, Spent};
use crate::state::{Membership, to_i16};

impl Game {
    /// Why the pupil cannot join `club` right now, or `None` if they can.
    #[must_use]
    pub fn join_block(&self, club: &ClubSpec) -> Option<String> {
        let p = &self.player;
        let slot = self.current_slot()?;
        if p.run.is_some() || p.life.lesson.is_some() || p.life.scene.is_some() {
            return Some("Not now.".into());
        }
        if p.life.rank_in(&club.id) > 0 {
            return Some("You already belong.".into());
        }
        if p.knows(&format!("quit_{}", club.id)) {
            return Some("They remember you walking out.".into());
        }
        let sign_up = club.join_place.as_deref().unwrap_or(&club.place);
        if p.place != sign_up {
            return Some(format!(
                "You sign up at the {}.",
                self.content
                    .place(sign_up)
                    .map_or("club", |x| x.name.as_str())
                    .to_lowercase()
            ));
        }
        if !self.place_open(sign_up, p.day, slot) {
            return Some("It is shut.".into());
        }
        if p.life.clubs.len() >= self.content.rules.max_clubs {
            return Some("You cannot give a third club the time.".into());
        }
        if let Some(rival) = p
            .life
            .clubs
            .iter()
            .find(|m| club.rivals.contains(&m.club))
            .and_then(|m| self.content.club(&m.club))
        {
            return Some(format!(
                "The {} would never forgive you.",
                rival.name.to_lowercase()
            ));
        }
        let unmet: Vec<String> = club
            .join
            .iter()
            .filter(|c| !self.holds(c))
            .map(crate::cond::Cond::describe)
            .collect();
        if unmet.is_empty() {
            None
        } else {
            Some(format!("They want {}.", unmet.join(" and ")))
        }
    }

    pub(crate) fn join_club(&mut self, id: &str) -> Result<Vec<Event>, Refused> {
        let club = self
            .content
            .club(id)
            .cloned()
            .ok_or_else(|| Refused::new("There is no such club."))?;
        if let Some(why) = self.join_block(&club) {
            return Err(Refused::new(why));
        }
        self.player.life.clubs.push(Membership {
            club: club.id.clone(),
            rank: 1,
            merit: 0,
            last_meet: -1,
        });
        // The faces of the club think better of someone who turns up.
        let mut events = vec![Event::Say(format!(
            "You join the {}: {}.",
            club.name,
            club.ranks[0].0.to_lowercase()
        ))];
        for member in &club.members {
            let clique = self.content.npc(member).and_then(|n| n.clique.clone());
            events.extend(self.change_bond(member, 1));
            if let Some(c) = clique {
                self.player.add_standing(&c, 1);
            }
        }
        events.extend(self.change_bond(&club.advisor, 1));
        Ok(events)
    }

    pub(crate) fn quit_club(&mut self, id: &str) -> Result<Vec<Event>, Refused> {
        let club = self
            .content
            .club(id)
            .cloned()
            .ok_or_else(|| Refused::new("There is no such club."))?;
        if self.player.life.rank_in(id) == 0 {
            return Err(Refused::new("You do not belong."));
        }
        if self.player.run.is_some()
            || self.player.life.lesson.is_some()
            || self.player.life.scene.is_some()
        {
            return Err(Refused::new("Not now."));
        }
        self.player.life.clubs.retain(|m| m.club != id);
        self.player.learn(&format!("quit_{id}"));
        for member in &club.members {
            if let Some(c) = self.content.npc(member).and_then(|n| n.clique.clone()) {
                self.player.add_standing(&c, -2);
            }
        }
        self.player.cool = to_i16(i32::from(self.player.cool) - 2);
        Ok(vec![Event::Say(format!(
            "You hand in your {} badge. Nobody says anything, which is worse.",
            club.name
        ))])
    }

    /// Whether a meeting of `club` is on right here, right now.
    #[must_use]
    pub fn meeting_on(&self, club: &ClubSpec) -> bool {
        let Some(slot) = self.current_slot() else {
            return false;
        };
        let day = self.player.day;
        self.kind(day, slot) == club.kind
            && (club.days.is_empty() || club.days.contains(&u8::try_from(Self::weekday(day)).unwrap_or(0)))
            && self.player.place == club.place
            && self.place_open(&club.place, day, slot)
    }

    pub(crate) fn meet(&mut self, rng: &mut DoorRng) -> Result<Vec<Event>, Refused> {
        let day = self.player.day;
        let club = self
            .content
            .clubs
            .iter()
            .find(|c| self.player.life.rank_in(&c.id) > 0 && self.meeting_on(c))
            .cloned()
            .ok_or_else(|| Refused::new("None of your clubs meets here and now."))?;
        if self.player.life.clubs.iter().any(|m| m.club == club.id && m.last_meet == day) {
            return Err(Refused::new("You have been to this one today."));
        }
        if self.kind(day, usize::from(self.player.slot)) == "after" && self.player.due > 0 {
            return Err(Refused::new("You owe a detention first."));
        }
        let slot = self.current_slot().unwrap_or(0);
        let mut events = vec![Event::Say(format!("The {} meets.", club.name))];
        let skill = self.skill_level(&club.skill);
        let merit = 1 + i32::from(skill >= 2);
        if let Some(m) = self.player.life.clubs.iter_mut().find(|m| m.club == club.id) {
            m.last_meet = day;
        }
        events.extend(self.add_merit(&club.id, merit));
        if let Some(skill_up) = self.gain_xp(&club.skill, 1) {
            events.push(Event::Say(format!("Your {skill_up} has improved.")));
        }
        events.extend(self.change_bond(&club.advisor, 1));
        let here: Vec<String> = self
            .npcs_at(&self.player.place, day, slot)
            .iter()
            .filter(|n| club.members.contains(&n.id))
            .map(|n| n.id.clone())
            .collect();
        for member in here {
            events.extend(self.change_bond(&member, 1));
        }
        events.extend(self.end_slot(Spent::Other, rng));
        Ok(events)
    }

    /// Add merit to a club the pupil belongs to and promote them if they have earned it.
    pub(crate) fn add_merit(&mut self, club_id: &str, merit: i32) -> Vec<Event> {
        let Some(club) = self.content.club(club_id).cloned() else {
            return Vec::new();
        };
        let Some(m) = self.player.life.clubs.iter_mut().find(|m| m.club == club_id) else {
            return Vec::new();
        };
        let total = (i32::from(m.merit) + merit).clamp(0, i32::from(u16::MAX));
        m.merit = u16::try_from(total).unwrap_or(0);
        let rank = club
            .ranks
            .iter()
            .filter(|(_, need)| total >= *need)
            .count()
            .max(1);
        let rank = u8::try_from(rank).unwrap_or(1);
        let mut events = Vec::new();
        if rank > m.rank {
            m.rank = rank;
            let name = club
                .ranks
                .get(usize::from(rank) - 1)
                .map_or("a higher rank", |r| r.0.as_str())
                .to_lowercase();
            events.push(Event::Say(format!(
                "The {} promotes you: {name}.",
                club.name
            )));
            self.player.cool = to_i16(i32::from(self.player.cool) + 2);
        }
        events
    }

    /// The clubs' term-end events: each club that made the pupil more than a newcomer rewards them.
    pub(crate) fn club_finales(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        let memberships = self.player.life.clubs.clone();
        for m in memberships {
            let Some(club) = self.content.club(&m.club).cloned() else {
                continue;
            };
            if m.rank < 2 {
                continue;
            }
            let legend = club.event_legend * (i32::from(m.rank) - 1);
            let _ = self.legend_gain(i64::from(legend));
            events.push(Event::Say(format!("{} (+{legend} legend)", club.event)));
        }
        events
    }
}
