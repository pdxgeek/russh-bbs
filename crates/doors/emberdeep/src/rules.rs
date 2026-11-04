//! The numbers the rules turn on. Content tables hold what exists; these hold how it plays, in
//! one place, so the balance harness can report on them and a later slice can tune them.

/// Torch-turns a hero has each day.
pub(crate) const TURNS_PER_DAY: u32 = 25;
/// Turns a night spent below costs the next day (the torches burned keeping watch).
pub(crate) const NIGHT_BELOW_TURNS: u32 = 8;
/// Days a fallen hero may be rescued before they wake in the temple.
pub(crate) const RECOVERY_DAYS: i64 = 3;
/// The deepest level (five bands of five).
pub(crate) const DEPTH_MAX: u8 = 25;
/// Levels in a depth band.
pub(crate) const BAND_LEVELS: u8 = 5;
/// The highest hero level.
pub(crate) const LEVEL_MAX: u8 = 20;
/// Skill points are earned up to this level (so a tree can never be completed).
pub(crate) const SKILL_POINT_LEVEL_MAX: u8 = 12;
/// An attribute never goes above this by points.
pub(crate) const ATTR_MAX: u8 = 16;
/// Free attribute points at creation.
pub(crate) const CREATION_POINTS: u8 = 4;
/// Traits chosen at creation.
pub(crate) const CREATION_TRAITS: usize = 2;
/// Turns a manual fight costs, and a quick-resolved one.
pub(crate) const FIGHT_TURNS: u32 = 2;
/// Turns a quick-resolved fight costs.
pub(crate) const QUICK_FIGHT_TURNS: u32 = 1;
/// Turns a camp rest costs.
pub(crate) const REST_TURNS: u32 = 2;
/// Turns carrying a fallen hero up costs.
pub(crate) const RESCUE_TURNS: u32 = 3;
/// Most scars a hero can carry.
pub(crate) const SCARS_MAX: usize = 3;
/// Most rounds of an auto-resolved fight before the hero breaks off.
pub(crate) const QUICK_ROUND_CAP: u32 = 40;
/// Salvage a hero carries before the climb back costs extra, per 10 over.
pub(crate) const BASE_CARRY: i32 = 20;

/// Experience needed in total to reach `level`.
pub(crate) fn xp_for_level(level: u8) -> u32 {
    let l = u32::from(level);
    20 * l * l.saturating_sub(1)
}
