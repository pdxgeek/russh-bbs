//! Backend-independent mapping for the widget lists (UI-10): how a member's ordered widget ids
//! are stored as one text and read back. Written once (D24).

use russh_bbs_core::terminal_settings::layout_from_stored;
use russh_bbs_core::widgets::{Choice, WidgetId};

/// The ids as stored: comma-joined, in order. An empty list is the empty text.
pub(crate) fn encode(list: &[WidgetId]) -> String {
    list.iter()
        .map(|id| id.as_str())
        .collect::<Vec<_>>()
        .join(",")
}

/// The ids in stored text. An id this build does not know (a newer build wrote it) is skipped,
/// not an error.
pub(crate) fn decode(text: &str) -> Vec<WidgetId> {
    text.split(',').filter_map(WidgetId::parse).collect()
}

/// A [`Choice`] from `(layout, widgets)` rows. A row naming a layout this build does not know
/// is skipped.
pub(crate) fn choice(rows: impl IntoIterator<Item = (String, String)>) -> Choice {
    Choice::from_lists(
        rows.into_iter()
            .filter_map(|(layout, widgets)| Some((layout_from_stored(&layout)?, decode(&widgets)))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_and_decode_round_trip_and_skip_the_unknown() {
        let list = [WidgetId::Clock, WidgetId::Mail, WidgetId::NewPosts];
        assert_eq!(encode(&list), "clock,mail,new_posts");
        assert_eq!(decode("clock,mail,new_posts"), list);
        assert_eq!(encode(&[]), "");
        assert_eq!(decode(""), []);
        assert_eq!(
            decode("clock,from_the_future,mail"),
            [WidgetId::Clock, WidgetId::Mail]
        );
    }
}
