//! The closed set of widgets the board can place (UI-10, D122).
//!
//! A widget is built in, in core and server code; it is not a plug-in. The id is a closed enum
//! so a stored list can only ever name something the code draws, and the stored text never
//! changes (add a variant, never rename one).

use crate::access::{Permission, Reach, ResourceKind, Snapshot};

/// One of the built-in widgets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum WidgetId {
    /// The board's name, tagline, location and age.
    Identity,
    /// The time and a month calendar.
    Clock,
    /// Members signed in now.
    Online,
    /// The last callers.
    Callers,
    /// Threads with new posts since the member's last call, per board.
    NewPosts,
    /// Unread mail.
    Mail,
    /// The chat rooms and how many are in each.
    Chat,
    /// Board activity over the last two weeks.
    Activity,
    /// The member's own credits and how they moved.
    Credits,
    /// Today's one-liners.
    Oneliner,
    /// The graffiti wall.
    Wall,
    /// The newest open poll.
    Poll,
    /// Headlines from the feed boards the member can read.
    News,
    /// The sysop's message of the day.
    Motd,
    /// A quote of the day.
    Quote,
    /// The doors the member can run, with their seasons.
    Doors,
    /// Sessions and traffic, for staff.
    Status,
}

impl WidgetId {
    /// Every widget, in the order the Widgets screen lists them.
    pub const ALL: [Self; 17] = [
        Self::Identity,
        Self::Clock,
        Self::Online,
        Self::Callers,
        Self::NewPosts,
        Self::Mail,
        Self::Chat,
        Self::Activity,
        Self::Credits,
        Self::Oneliner,
        Self::Wall,
        Self::Poll,
        Self::News,
        Self::Motd,
        Self::Quote,
        Self::Doors,
        Self::Status,
    ];

    /// The stable text stored and written in config. Never rename a value.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Identity => "identity",
            Self::Clock => "clock",
            Self::Online => "online",
            Self::Callers => "callers",
            Self::NewPosts => "new_posts",
            Self::Mail => "mail",
            Self::Chat => "chat",
            Self::Activity => "activity",
            Self::Credits => "credits",
            Self::Oneliner => "oneliner",
            Self::Wall => "wall",
            Self::Poll => "poll",
            Self::News => "news",
            Self::Motd => "motd",
            Self::Quote => "quote",
            Self::Doors => "doors",
            Self::Status => "status",
        }
    }

    /// Parse the stored or configured text.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|id| id.as_str() == text)
    }

    /// Whether the viewer may be shown this widget at all: the same permission, and the same
    /// resources, as the screen the widget summarises. A widget the viewer cannot reach is not
    /// offered, not drawn and not fetched, so it can never show what the viewer could not open.
    #[must_use]
    pub fn reachable_by(self, access: &Snapshot) -> bool {
        let global = |permission| access.resolve(permission, None).is_granted();
        let some = |permission, kind| access.reach(permission, kind) != Reach::None;
        // A waiting-room key holds none of these, and the account check is a second guard.
        if access.account().is_none() {
            return false;
        }
        match self {
            Self::Identity | Self::Clock | Self::Motd | Self::Quote | Self::Credits => true,
            Self::Online => global(Permission::MembersView),
            Self::Callers | Self::Oneliner | Self::Wall | Self::Poll => {
                global(Permission::CommunityView)
            }
            Self::NewPosts | Self::Activity | Self::News => {
                some(Permission::ForumList, ResourceKind::Forum)
            }
            Self::Mail => global(Permission::MailRead),
            Self::Chat => some(Permission::ChatJoin, ResourceKind::Room),
            Self::Doors => global(Permission::DoorsRun),
            Self::Status => global(Permission::SessionsView),
        }
    }
}
