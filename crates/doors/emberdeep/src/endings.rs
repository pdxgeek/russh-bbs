//! Seasons and endings (M4-E4). When the Cold Furnace's guardian falls, its slayer gives a
//! **verdict** on the fire (a choice their questlines may have opened or closed), and the
//! night's tick turns the verdict and what the whole town did into one of six **collective
//! endings**, written in the chronicle. Each hero also has an **epilogue**, drawn from their own
//! deeds and choices. The dungeon then collapses into a new layout for the next epoch.

use crate::content::Content;
use crate::hero::Hero;
use crate::world::World;

/// A verdict the slayer may give.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Verdict {
    pub(crate) id: &'static str,
    pub(crate) label: &'static str,
    pub(crate) text: &'static str,
    /// A choice in a questline the hero must have made, if the verdict is closed to others.
    pub(crate) needs_flag: Option<&'static str>,
}

/// The verdicts, in menu order. Quenching is open to everyone; the rest follow from choices.
pub(crate) const VERDICTS: [Verdict; 4] = [
    Verdict {
        id: "quench",
        label: "Put the fire out",
        text: "You let the Cold Furnace go cold for good.",
        needs_flag: None,
    },
    Verdict {
        id: "bank",
        label: "Bank the fire for the town",
        text: "You carry a coal up the stair, and bank it under the temple.",
        needs_flag: Some("keeps-the-flame"),
    },
    Verdict {
        id: "claim",
        label: "Take the heart for yourself",
        text: "You close your hand around the heart of the Furnace.",
        needs_flag: Some("keeps-the-key"),
    },
    Verdict {
        id: "open",
        label: "Open the Furnace to everyone",
        text: "You throw the old gates wide, and the deep air rushes out.",
        needs_flag: Some("opens-the-gate"),
    },
];

/// Every flag a verdict needs (the quest tables must offer a choice that sets each).
pub(crate) fn verdict_flags() -> impl Iterator<Item = &'static str> {
    VERDICTS.iter().filter_map(|v| v.needs_flag)
}

pub(crate) fn verdict(id: &str) -> Option<&'static Verdict> {
    VERDICTS.iter().find(|v| v.id == id)
}

/// How a fall of the Furnace ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Ending {
    pub(crate) name: &'static str,
    pub(crate) text: &'static str,
}

/// The collective endings, by code.
pub(crate) const ENDINGS: [Ending; 6] = [
    Ending {
        name: "The Hearth Rekindled",
        text: "The banked coal takes, and Cindermere is warm for the first time in a hundred winters.",
    },
    Ending {
        name: "Embers Banked",
        text: "A small flame lives under the temple. The town is warmer, and still waiting on itself.",
    },
    Ending {
        name: "The Ember Crown",
        text: "One hero holds the heart, and the town learns what it is to have a warden it did not choose.",
    },
    Ending {
        name: "The Open Gate",
        text: "The lifts run and the gates stand open; the Emberdeep becomes a road, and then a market.",
    },
    Ending {
        name: "The Long Table",
        text: "The fire goes out, but the heroes who carried each other home keep a long table set, and the town does not go cold.",
    },
    Ending {
        name: "The Cold Furnace",
        text: "The fire goes out. Cindermere winters hard, and tells the story of those who put it out.",
    },
];

/// Projects the town must have finished for the banked fire to take hold.
pub(crate) const REKINDLE_PROJECTS: usize = 8;
/// Ways down (shortcuts and lifts) the open gate needs.
pub(crate) const OPEN_GATE_WAYS: usize = 6;
/// Companions carried home that make a long table.
pub(crate) const LONG_TABLE_RESCUES: u16 = 5;

/// The collective ending of the season's fall, from the verdict and the town's deeds.
pub(crate) fn collective(world: &World, content: &Content) -> u8 {
    let kind = world
        .commons
        .verdict
        .as_ref()
        .map_or("quench", |(k, _)| k.as_str());
    let done = content
        .projects
        .project
        .iter()
        .filter(|p| world.is_done(content, &p.id))
        .count();
    let ways = world.ways_down(content).len();
    match kind {
        "claim" => 2,
        "bank" if done >= REKINDLE_PROJECTS => 0,
        "bank" => 1,
        "open" if ways >= OPEN_GATE_WAYS => 3,
        _ if world.commons.rescues >= LONG_TABLE_RESCUES
            && world.commons.parties.iter().any(|p| p.rescues > 0) =>
        {
            4
        }
        _ => 5,
    }
}

/// How one hero is remembered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Epilogue {
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    pub(crate) text: &'static str,
}

/// Every epilogue, most notable first.
pub(crate) const EPILOGUES: [Epilogue; 8] = [
    Epilogue {
        id: "furnace",
        name: "Bearer of the Furnace",
        text: "You were the one who stood before the Cold Furnace, and the town will not forget the answer you gave.",
    },
    Epilogue {
        id: "champion",
        name: "Champion of the Ring",
        text: "You won more than you lost under the ring's lamps, and the young still learn your feints.",
    },
    Epilogue {
        id: "hearth",
        name: "Hearth-Bearer",
        text: "You carried others home. They keep a place for you at every fire.",
    },
    Epilogue {
        id: "patron",
        name: "Patron of Cindermere",
        text: "Your coin is in the stones of the town works, and the children learn your name from the plaques.",
    },
    Epilogue {
        id: "flame",
        name: "Keeper of the Low Flame",
        text: "You promised the lamp a share of everything, and it has not failed you yet.",
    },
    Epilogue {
        id: "deep",
        name: "Deep Delver",
        text: "You have seen the lower halls. Few come back who can say that.",
    },
    Epilogue {
        id: "scarred",
        name: "The Scarred",
        text: "The Emberdeep wrote on you, and you wear it the way others wear a medal.",
    },
    Epilogue {
        id: "hand",
        name: "A Hand in the Dark",
        text: "You came, you carried a lamp, and the dark was a little less for it.",
    },
];

/// The epilogue for a hero, from their own deeds and choices.
pub(crate) fn epilogue(hero: &Hero) -> &'static Epilogue {
    let given: u32 = hero.given.iter().map(|(_, c, _)| *c).sum();
    let id = if hero.story.guardians & (1 << 4) != 0 {
        "furnace"
    } else if hero.social.wins >= 5 && hero.social.wins > hero.social.losses {
        "champion"
    } else if hero.social.rescues >= 3 {
        "hearth"
    } else if given >= 500 {
        "patron"
    } else if hero.story.flags.iter().any(|f| f == "keeps-the-flame") {
        "flame"
    } else if hero.best_depth >= 20 {
        "deep"
    } else if hero.scars.len() >= 2 {
        "scarred"
    } else {
        "hand"
    };
    EPILOGUES
        .iter()
        .find(|e| e.id == id)
        .unwrap_or(&EPILOGUES[7])
}
