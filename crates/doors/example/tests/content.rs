//! The schema test for the door's content tables (the framework's validation hook).

use russh_bbs_door::content::{self, ContentError};
use russh_bbs_door_example::content::{PHRASES_TOML, Phrases};

#[test]
fn the_embedded_table_passes_its_schema() {
    assert_eq!(content::check::<Phrases>("phrases", PHRASES_TOML), Ok(()));
    let phrases = Phrases::embedded();
    assert!(phrases.win.len() >= 3 && phrases.lose.len() >= 3);
}

#[test]
fn a_bad_edit_is_named_not_swallowed() {
    let duplicate = "[[win]]\nid='a'\ntext='x'\n[[win]]\nid='a'\ntext='y'\n[[lose]]\nid='b'\ntext='z'\n";
    match content::check::<Phrases>("phrases", duplicate) {
        Err(ContentError::Invalid { problems, .. }) => {
            assert!(problems.iter().any(|p| p.contains("used twice")), "{problems:?}");
        }
        other => panic!("{other:?}"),
    }
    let escape = "[[win]]\nid='a'\ntext=\"\\u001b[31mred\"\n[[lose]]\nid='b'\ntext='z'\n";
    assert!(matches!(
        content::check::<Phrases>("phrases", escape),
        Err(ContentError::Invalid { .. })
    ));
    assert!(matches!(
        content::check::<Phrases>("phrases", "[[win]]\nid='a'\n"),
        Err(ContentError::Parse { .. })
    ));
    let no_lose = "[[win]]\nid='a'\ntext='x'\n";
    assert!(content::check::<Phrases>("phrases", no_lose).is_err());
}

#[test]
fn the_door_id_is_a_valid_door_id() {
    assert!(russh_bbs_door::DoorId::parse(russh_bbs_door_example::DOOR_ID).is_ok());
}
