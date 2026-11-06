//! Unit tests for the engine, grouped by what they prove.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::too_many_lines,
    clippy::many_single_char_names,
    clippy::match_same_arms,
    clippy::assert_is_empty,
    clippy::single_match_else,
    clippy::int_plus_one,
    clippy::neg_multiply,
    clippy::map_unwrap_or,
    clippy::manual_assert_eq
)]

mod balance;
mod board;
mod branches;
mod combat;
mod content_town;
mod death;
mod dungeon;
mod economy;
mod gear;
mod hero;
mod legacy;
mod projects;
mod saves;
mod social;
mod support;
mod town;
mod world;
