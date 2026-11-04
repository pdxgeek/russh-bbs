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
            rects.extend(plan.slots.iter().map(|(_, r)| *r));
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
    assert_eq!(dashboard.slots.len(), 6, "a 2x3 grid at this height");
    let wide = plan(LayoutId::Wide, body);
    assert!(wide.content.is_some(), "three columns");
    assert!(wide.nav.x < wide.content.unwrap().x && wide.content.unwrap().x < wide.slots[0].1.x);
    let split = plan(LayoutId::Split, body);
    assert!(split.divider.is_some());
    assert_eq!(split.slots.len(), 1);
    // The classic family has no panes of its own.
    for layout in [
        LayoutId::Classic,
        LayoutId::Lightbar,
        LayoutId::Compact,
        LayoutId::Focus,
    ] {
        let p = plan(layout, body);
        assert!(p.slots.is_empty() && p.content.is_none() && p.divider.is_none());
    }
}

#[test]
fn a_short_dashboard_drops_rows_instead_of_squashing_them() {
    let tall = plan(LayoutId::Dashboard, Rect::new(0, 1, 80, 22))
        .slots
        .len();
    let short = plan(LayoutId::Dashboard, Rect::new(0, 1, 80, 14))
        .slots
        .len();
    let tiny = plan(LayoutId::Dashboard, Rect::new(0, 1, 80, 8))
        .slots
        .len();
    assert_eq!((tall, short, tiny), (6, 4, 2));
}

#[test]
fn every_slot_has_a_title_key() {
    for slot in [
        Slot::Identity,
        Slot::Clock,
        Slot::WhoOnline,
        Slot::LastCallers,
        Slot::NewPosts,
        Slot::UnreadMail,
        Slot::Chat,
        Slot::Activity,
    ] {
        assert!(slot.title_key().starts_with("ui.slot."));
    }
}
