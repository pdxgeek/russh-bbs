#![allow(clippy::unwrap_used, clippy::expect_used)]

use ratatui::layout::Rect;

use super::*;

fn body(width: u16, height: u16) -> Rect {
    // A body starts under the header, so it never begins at row 0 or ends on the last row.
    Rect::new(0, 1, width, height.saturating_sub(2))
}

#[test]
fn bounds_keep_both_minimums() {
    for total in 0..=200_u16 {
        match bounds(total) {
            None => assert!(total < MIN_MAIN + DIVIDER + MIN_PANE, "{total}"),
            Some((least, most)) => {
                assert_eq!(least, MIN_PANE);
                assert!(most >= least);
                assert_eq!(total - most - DIVIDER, MIN_MAIN, "{total}");
            }
        }
    }
}

#[test]
fn split_is_disjoint_inside_the_body_and_covers_it() {
    for width in 0..=200_u16 {
        for wanted in [0_u16, 1, 24, 30, 52, 100, 250, u16::MAX] {
            let area = body(width, 24);
            let Some(geometry) = split(area, wanted) else {
                assert!(bounds(width).is_none(), "{width}");
                continue;
            };
            let Geometry {
                main,
                divider,
                pane,
            } = geometry;
            assert!(main.width >= MIN_MAIN, "{width} {wanted}");
            assert!(pane.width >= MIN_PANE, "{width} {wanted}");
            assert_eq!(divider.width, DIVIDER);
            // Side by side, in order, with no gap and no overlap.
            assert_eq!(main.x, area.x);
            assert_eq!(divider.x, main.x + main.width);
            assert_eq!(pane.x, divider.x + divider.width);
            assert_eq!(pane.x + pane.width, area.x + area.width);
            for part in [main, divider, pane] {
                assert_eq!((part.y, part.height), (area.y, area.height));
                assert!(area.contains((part.x, part.y).into()));
            }
        }
    }
}

#[test]
fn no_part_reaches_the_header_row_or_the_status_row() {
    // The terminal is 24 rows: row 0 is the header, row 23 the status bar with the marker cell.
    let area = body(80, 24);
    let geometry = split(area, 30).expect("80 columns hold a dock");
    for part in [geometry.main, geometry.divider, geometry.pane] {
        assert!(part.y >= 1, "{part:?}");
        assert!(part.y + part.height <= 23, "{part:?}");
    }
}

#[test]
fn clamp_holds_the_wanted_width_between_the_bounds() {
    let (least, most) = bounds(80).expect("80 columns hold a dock");
    assert_eq!(clamp(80, 0), Some(least));
    assert_eq!(clamp(80, u16::MAX), Some(most));
    assert_eq!(clamp(80, 30), Some(30));
    assert_eq!(clamp(60, 30), None);
}

#[test]
fn the_default_width_is_inside_the_bounds_at_the_sizes_we_test() {
    for total in [65_u16, 80, 100, 132, 200] {
        let (least, most) = bounds(total).expect("wide enough");
        let width = default_width(total);
        assert!((least..=most).contains(&width), "{total}: {width}");
    }
}

#[test]
fn resize_keys_move_the_divider_and_stop_at_the_bounds() {
    let (least, most) = bounds(80).expect("80 columns hold a dock");
    assert_eq!(resized(80, 30, Key::Char('<')), Some(31));
    assert_eq!(resized(80, 30, Key::Char('>')), Some(29));
    assert_eq!(resized(80, 30, Key::Char('{')), Some(38));
    assert_eq!(resized(80, 30, Key::Char('}')), Some(22.max(least)));
    assert_eq!(resized(80, least, Key::Char('>')), Some(least));
    assert_eq!(resized(80, most, Key::Char('<')), Some(most));
    assert_eq!(resized(80, most, Key::Char('{')), Some(most));
    assert_eq!(resized(80, 30, Key::Char('x')), None);
    assert_eq!(resized(80, 30, Key::Enter), None);
    // Repeated keys never leave the bounds.
    let mut width = 30;
    for _ in 0..500 {
        width = resized(80, width, Key::Char('<')).expect("a resize key");
    }
    assert_eq!(width, most);
    for _ in 0..500 {
        width = resized(80, width, Key::Char('>')).expect("a resize key");
    }
    assert_eq!(width, least);
}

#[test]
fn a_drag_sets_the_width_that_puts_the_divider_on_the_column() {
    let area = body(80, 24);
    // A pane of 30 has its divider at column 80 - 30 - 1 = 49.
    assert_eq!(drag_target(area, 49), Some(30));
    assert_eq!(drag_target(area, 40), Some(39));
    assert_eq!(drag_target(area, 55), Some(24));
}

#[test]
fn a_drag_past_either_edge_stops_at_the_bounds() {
    let area = body(80, 24);
    let (least, most) = bounds(80).expect("80 columns hold a dock");
    assert_eq!(drag_target(area, 0), Some(most));
    assert_eq!(drag_target(area, 500), Some(least));
    assert_eq!(drag_target(body(60, 24), 30), None);
}

#[test]
fn carve_shrinks_only_the_requested_body_and_reports_what_it_reserved() {
    let area = body(80, 24);
    // No request: the body is untouched.
    assert_eq!(carve(area), area);
    let (inside, used) = with(Some(30), || carve(area));
    let geometry = used.expect("a dock was reserved");
    assert_eq!(inside, geometry.main);
    assert_eq!(geometry.pane.width, 30);
    // Only the first body of a draw is carved.
    let (second, _) = with(Some(30), || {
        let _ = carve(area);
        carve(area)
    });
    assert_eq!(second, area);
    // Too narrow: nothing reserved.
    let narrow = body(60, 24);
    let (inside, used) = with(Some(30), || carve(narrow));
    assert_eq!(inside, narrow);
    assert!(used.is_none());
    // The request does not outlive the draw.
    assert_eq!(carve(area), area);
}

#[test]
fn main_width_matches_the_drawn_screen() {
    for total in [50_u16, 65, 80, 132] {
        for wanted in [10_u16, 30, 60, 200] {
            let area = body(total, 24);
            let drawn = split(area, wanted).map_or(area.width, |g| g.main.width);
            assert_eq!(main_width(total, wanted), drawn, "{total} {wanted}");
        }
    }
}
