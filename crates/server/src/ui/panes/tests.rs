#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::type_complexity,
    clippy::panic,
    clippy::assert_is_empty
)]

use super::*;

/// Sizes the geometry is checked at, from tiny to ultrawide.
const SIZES: [(u16, u16); 9] = [
    (24, 9),
    (44, 12),
    (72, 16),
    (80, 22),
    (100, 28),
    (110, 22),
    (132, 41),
    (200, 60),
    (500, 198),
];

fn inside(inner: Rect, outer: Rect) -> bool {
    inner.x >= outer.x
        && inner.y >= outer.y
        && inner.x + inner.width <= outer.x + outer.width
        && inner.y + inner.height <= outer.y + outer.height
}

#[test]
fn panes_stay_inside_the_body_and_never_overlap() {
    for layout in LayoutId::ALL {
        for (w, h) in SIZES {
            let body = Rect::new(0, 1, w, h);
            let plan = plan(layout, body);
            let mut rects = vec![plan.nav];
            rects.extend(plan.content);
            rects.extend(plan.divider);
            rects.extend(plan.cells.iter().copied());
            for rect in &rects {
                assert!(
                    inside(*rect, body),
                    "{layout:?} {w}x{h}: {rect:?} leaves {body:?}"
                );
            }
            for (i, a) in rects.iter().enumerate() {
                for b in &rects[i + 1..] {
                    let overlap = a.intersection(*b);
                    assert!(
                        overlap.width == 0 || overlap.height == 0,
                        "{layout:?} {w}x{h}: {a:?} overlaps {b:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn the_pane_layouts_are_real_arrangements() {
    let body = Rect::new(0, 1, 132, 41);
    let dashboard = plan(LayoutId::Dashboard, body);
    assert_eq!(dashboard.cells.len(), 12, "three columns of four rows at this size");
    let wide = plan(LayoutId::Wide, body);
    assert!(wide.content.is_some(), "three columns");
    assert!(wide.nav.x < wide.content.unwrap().x && wide.content.unwrap().x < wide.cells[0].x);
    assert!(wide.cells.len() >= 4, "a tall rail holds several widgets");
    let split = plan(LayoutId::Split, body);
    assert!(split.divider.is_some());
    assert!(split.cells.len() >= 2);
    // The classic family has no panes of its own.
    for layout in [
        LayoutId::Classic,
        LayoutId::Lightbar,
        LayoutId::Compact,
        LayoutId::Focus,
    ] {
        let p = plan(layout, body);
        assert!(p.cells.is_empty() && p.content.is_none() && p.divider.is_none());
    }
}

#[test]
fn a_short_dashboard_drops_rows_instead_of_squashing_them() {
    let cells = |h| plan(LayoutId::Dashboard, Rect::new(0, 1, 80, h)).cells.len();
    // Two columns at 80 wide; rows grow with the height.
    assert_eq!((cells(22), cells(14), cells(8)), (6, 4, 2));
    // Three columns once the grid is 90 wide.
    assert_eq!(plan(LayoutId::Dashboard, Rect::new(0, 1, 132, 22)).cells.len(), 9);
}

#[test]
fn a_cell_is_tall_enough_for_a_panel_with_content() {
    // Every cell the layouts offer holds a panel with at least a few rows inside it.
    for layout in [LayoutId::Dashboard, LayoutId::Wide, LayoutId::Split] {
        for (w, h) in [(80, 21), (100, 28), (132, 40), (200, 60)] {
            for cell in plan(layout, Rect::new(0, 1, w, h)).cells {
                if w >= 80 && h >= 21 {
                    assert!(cell.height >= 4, "{layout:?} {w}x{h}: {cell:?}");
                }
            }
        }
    }
}
