use super::*;

fn config(edit: impl FnOnce(&mut BoardConfig)) -> Result<Board, BoardError> {
    let mut config = BoardConfig::default();
    edit(&mut config);
    Board::from_config(&config)
}

#[test]
fn the_defaults_are_the_words_the_screens_always_had() {
    let board = Board::default();
    assert_eq!(board.name(), "russh-bbs");
    assert_eq!(board.title_override(), None, "nothing to override");
    assert!(board.location().is_empty() && board.since().is_empty());
    assert!(board.accent().is_none() && board.style().is_none() && board.layout().is_none());
    assert!(board.styles().is_empty() && board.layouts().is_empty());
    assert!(board.motion());
}

#[test]
fn a_full_section_is_accepted() {
    let board = config(|c| {
        c.name = "Night Owl BBS".into();
        c.tagline = "After hours".into();
        c.location = "Portland".into();
        c.since = "2026".into();
        c.accent = Some("#ff00aa".into());
        c.style = Some("neon".into());
        c.layout = Some("dashboard".into());
        c.styles = "neon, paper ,ansi".into();
        c.layouts = "classic,dashboard".into();
        c.motion = false;
    })
    .unwrap();
    assert_eq!(board.name(), "Night Owl BBS");
    assert_eq!(board.title_override().as_deref(), Some("Night Owl BBS"));
    assert_eq!(board.accent().and_then(Tint::rgb), Some((255, 0, 170)));
    assert_eq!(board.styles().len(), 3);
    assert_eq!(board.layouts(), [LayoutId::Classic, LayoutId::Dashboard]);
    assert!(!board.motion());
}

#[test]
fn bad_values_are_refused_with_the_key_that_is_wrong() {
    let cases: Vec<(&str, Box<dyn FnOnce(&mut BoardConfig)>)> = vec![
        ("board.name", Box::new(|c| c.name = "  ".into())),
        (
            "board.name",
            Box::new(|c| c.name = "x".repeat(NAME_MAX + 1)),
        ),
        (
            "board.name",
            Box::new(|c| c.name = "esc\u{1b}[31mred".into()),
        ),
        (
            "board.name",
            Box::new(|c| c.name = "zero\u{200b}width".into()),
        ),
        (
            "board.name",
            Box::new(|c| c.name = "fake \u{27e6}x\u{27e7}".into()),
        ),
        (
            "board.name",
            Box::new(|c| c.name = "nerd \u{f015} glyph".into()),
        ),
        ("board.name", Box::new(|c| c.name = "two\nlines".into())),
        (
            "board.tagline",
            Box::new(|c| c.tagline = "y".repeat(LINE_MAX + 1)),
        ),
        (
            "board.since",
            Box::new(|c| c.since = "s".repeat(SINCE_MAX + 1)),
        ),
        ("board.accent", Box::new(|c| c.accent = Some("#12".into()))),
        (
            "board.accent",
            Box::new(|c| c.accent = Some("chartreuse".into())),
        ),
        ("board.accent", Box::new(|c| c.accent = Some("300".into()))),
        (
            "board.style",
            Box::new(|c| c.style = Some("Bad Name".into())),
        ),
        (
            "board.layout",
            Box::new(|c| c.layout = Some("tiled".into())),
        ),
        ("board.styles", Box::new(|c| c.styles = "neon,neon".into())),
        (
            "board.styles",
            Box::new(|c| c.styles = "neon,,".to_owned() + &"x,".repeat(40)),
        ),
        (
            "board.layouts",
            Box::new(|c| c.layouts = "classic,tiled".into()),
        ),
        (
            "board.style",
            Box::new(|c| {
                c.style = Some("neon".into());
                c.styles = "paper".into();
            }),
        ),
        (
            "board.layout",
            Box::new(|c| {
                c.layout = Some("wide".into());
                c.layouts = "classic".into();
            }),
        ),
    ];
    for (key, edit) in cases {
        let error = config(edit).expect_err(key);
        assert_eq!(error.key, key, "{error}");
        assert!(!error.message.is_empty());
    }
}

#[test]
fn a_board_cannot_be_named_to_claim_privacy() {
    for name in [
        "Secure BBS",
        "The Private Room",
        "Encrypted Chat",
        "E2EE hall",
    ] {
        let error = config(|c| c.name = name.into()).expect_err(name);
        assert_eq!(error.key, "board.name");
        assert!(error.message.contains("PGP"), "{error}");
    }
    assert!(config(|c| c.tagline = "totally secure".into()).is_err());
    assert!(config(|c| c.location = "a \u{1f510} vault".into()).is_err());
}

#[test]
fn a_color_can_be_a_name_an_index_or_hex() {
    for ok in ["#00ffcc", "lightmagenta", "208"] {
        assert!(config(|c| c.accent = Some(ok.into())).is_ok(), "{ok}");
    }
}
