//! The append-only promise: saves and the world address these tables **by position** (an open
//! scene stores an event's index, a fight an enemy's, a boarding a layout's, the world a news
//! kind's and a convoy event's, a ship its modules and goods). Rows may be added at the end and
//! never reordered, renamed or removed. This test pins every id at its index as of M4-L4; when
//! you append, add the new ids to the end of the list here.
#![allow(clippy::unwrap_used, missing_docs)]

use russh_bbs_door_long_haul::content::Content;

fn check(what: &str, now: &[&str], pinned: &[&str]) {
    assert!(
        now.len() >= pinned.len(),
        "{what}: rows were removed ({} now, {} pinned)",
        now.len(),
        pinned.len()
    );
    for (i, id) in pinned.iter().enumerate() {
        assert_eq!(now[i], *id, "{what}: row {i} changed (append only)");
    }
}

const EVENTS: &[&str] = &[
    "dead_freighter",
    "medical_beacon",
    "lifepod",
    "false_distress",
    "dry_tug",
    "station_breach",
    "pilgrim_air",
    "navy_call",
    "trapped_miners",
    "skiff_toll",
    "debris_ambush",
    "captains_colours",
    "corsair_hunt",
    "rail_pass",
    "claim_jumpers",
    "press_offer",
    "prisoner_swap",
    "bounty_quarry",
    "customs_sweep",
    "union_blockade",
    "navy_hail",
    "guild_audit",
    "church_tithe",
    "early_warning",
    "smugglers_lane",
    "coolant_leak",
    "phase_stutter",
    "overheat",
    "fuel_leak",
    "drive_whisper",
    "cracked_coil",
    "gate_song",
    "ghost_log",
    "dark_beacon",
    "numbers_station",
    "gate_ruin",
    "nebula_pulse",
    "guild_relay",
    "hollow_voice",
    "micrometeor_storm",
    "radiation_flare",
    "gas_pocket",
    "tidal_shear",
    "pilgrim_hymn",
    "scholar_demand",
    "fugitive_hunted",
    "family_fever",
    "officer_sees",
    "deserter_known",
    "ore_barge_drift",
    "quarantine_ship",
    "navy_hulk",
    "galley_brawl",
    "stowaway_found",
    "homesick",
    "drifting_pods",
    "roving_trader",
    "guild_buyer",
    "black_ship",
    "navigators_rhyme",
    "dead_tanker_a",
    "fever_hauler_a",
    "stranded_survey_a",
    "mayday_trap_a",
    "lantern_barge_a",
    "claim_boat_leak_a",
    "marauder_ledger_a",
    "crate_collectors_a",
    "pressed_hands_a",
    "dice_captain_a",
    "old_enemies_a",
    "hostage_lifeboat_a",
    "picket_line_a",
    "customs_hawk_a",
    "weigh_station_a",
    "gate_toll_a",
    "shaft_shriek_a",
    "coil_chorus_a",
    "ghost_thrust_a",
    "dirty_injectors_a",
    "rod_fever_a",
    "relay_tap_a",
    "lantern_signal_a",
    "pilgrim_chant_a",
    "echo_manifest_a",
    "warden_challenge_a",
    "cartel_hum_a",
    "blank_chart_a",
    "ion_squall_b",
    "mine_belt_b",
    "gravity_wake_b",
    "fouled_coolant_b",
    "gate_scar_b",
    "sleeper_comet_b",
    "refugee_rations_b",
    "officer_requisition_b",
    "miner_child_fever_b",
    "deserter_picket_b",
    "fugitive_truth_b",
    "scholar_archive_b",
    "ghost_liner_b",
    "cargo_ghost_b",
    "medical_frigate_b",
    "quarantine_cutter_b",
    "reliquary_b",
    "tomb_barge_b",
    "prison_hulk_b",
    "race_yacht_b",
    "drone_nest_b",
    "old_claim_b",
    "watch_rota_b",
    "night_terrors_b",
    "wage_petition_b",
    "stash_buoy_b",
    "union_toll_b",
    "customs_hawk_b",
    "water_cartel_b",
    "pilgrim_barge_b",
    "warden_lights_b",
    "dead_reckoning_b",
];

const ARC_SCENES: &[&str] = &[
    "gates_1_flat",
    "gates_2_vigil",
    "gates_3_ore",
    "gates_4_ledger",
    "gates_5_override",
    "gates_6_charts",
    "gates_7_wardens",
    "gates_8_loom",
    "gates_9_flags",
    "gates_10_song",
    "league_1_muster",
    "league_2_beat",
    "league_3_blockade",
    "league_4_cadet",
    "league_5_haven",
    "league_6_inquiry",
    "league_7_flare",
    "league_8_colours",
    "union_1_hall",
    "union_2_supply",
    "union_3_scab",
    "union_4_shaft",
    "union_5_offer",
    "union_6_vote",
    "union_7_cordon",
    "union_8_strike",
    "free_1_debt",
    "free_2_toll",
    "free_3_widow",
    "free_4_picket",
    "free_5_hands",
    "free_6_raid",
    "free_7_moot",
    "free_8_ledger",
    "guild_1_gap",
    "guild_2_buoy",
    "guild_3_pell",
    "guild_4_system",
    "guild_5_auditor",
    "guild_6_ring",
    "guild_7_blank",
    "church_1_candles",
    "church_2_vigil",
    "church_3_opened",
    "church_4_reliquary",
    "church_5_quill",
    "church_6_wardens",
    "church_7_hum",
    "church_8_rite",
];

const DERELICTS: &[&str] = &[
    "pirate_wreck",
    "corsair_wreck",
    "ore_barge",
    "plague_ship",
    "survey_cutter",
    "gate_ark",
    "navy_hulk",
    "sentinel_husk",
    "claim_station",
    "cargo_ghost",
    "medical_frigate",
    "mining_platform",
    "pilgrim_barge",
    "survey_buoy_tender",
    "smuggler_stash",
    "league_picket",
    "union_tug",
    "church_reliquary",
    "guild_archive",
    "colony_seedship",
    "prison_hulk",
    "racing_yacht",
    "scrap_crawler",
    "dead_refinery",
    "fuel_tanker",
    "comms_relay",
    "gunship_hulk",
    "hermit_ship",
    "ghost_liner",
    "salvage_tug",
    "derelict_casino",
    "quarantine_cutter",
    "courier_wreck",
    "ice_hauler",
    "arc_lab",
    "drone_nest",
    "outrider_wreck",
    "mutiny_ship",
    "tomb_barge",
    "gate_fragment",
];

const ENEMIES: &[&str] = &[
    "skiff",
    "raider",
    "corsair",
    "railer",
    "jumper",
    "cutter",
    "gunboat",
    "enforcer",
    "sentinel",
    "marauder",
    "blockade_runner",
    "picket",
    "customs_hawk",
    "union_marshal",
    "gate_warden",
];

const NEWS: &[&str] = &[
    "plague",
    "strike",
    "collapse",
    "harvest",
    "blight",
    "war",
    "blockade",
    "rush",
    "scare",
    "festival",
    "leak",
    "recall",
    "burst",
    "crackdown",
    "raid",
    "flare",
    "relics",
    "amnesty",
    "skim",
    "frost",
];

const MODULES: &[&str] = &[
    "ion1", "ion2", "ion3", "long1", "pod1", "pod2", "tank1", "tank2", "scan1", "scan2", "scan3",
    "lance1", "shield1", "shield2", "cabin1", "refine1", "lab1", "lance2", "rail1", "scatter1",
    "scrap1", "haul1", "ion2r", "sprint1", "long2", "sprint2", "pod0", "rack1", "cold1", "pod3",
    "tank0", "tank1s", "tank2r", "tank3", "scan0", "scan2s", "weld0", "needle1", "flak2",
    "plasma3", "rail3", "plate0", "plate2", "bastion3", "bunk0", "suite2", "skim0", "skim2",
    "bench0", "field2",
];

const CONVOY_EVENTS: &[&str] = &[
    "tailwind",
    "dust_cloud",
    "raiders",
    "salvage_lane",
    "toll_gate",
    "shared_fire",
];

const GOODS: &[&str] = &[
    "water",
    "ore",
    "volatiles",
    "grain",
    "vat_protein",
    "salt_spice",
    "textiles",
    "alloy",
    "parts",
    "coils",
    "circuits",
    "sensor_glass",
    "medicine",
    "antitoxin",
    "biogel",
    "seed_stock",
    "lab_samples",
    "data_cores",
    "ceramics",
    "polymers",
    "rare_earths",
    "tools",
    "reactor_rods",
    "glass_wine",
    "curios",
    "ghostwire",
    "dreamsilt",
    "stripped_cores",
    "gate_relics",
    "slug_guns",
];

const JOB_PASSENGERS: &[&str] = &[
    "pilgrim",
    "scholar",
    "fugitive",
    "deserter",
    "miner_family",
    "officer",
    "refugee",
];

#[test]
fn every_table_addressed_by_index_is_append_only() {
    let c = Content::embedded().unwrap();
    let transit: Vec<&str> = c.transit.event.iter().map(|e| e.id.as_str()).collect();
    let mut want_events: Vec<&str> = EVENTS.to_vec();
    want_events.extend_from_slice(ARC_SCENES);
    check("transit events then arc scenes", &transit, &want_events);
    check(
        "derelicts",
        &c.derelicts
            .derelict
            .iter()
            .map(|d| d.id.as_str())
            .collect::<Vec<_>>(),
        DERELICTS,
    );
    check(
        "enemies",
        &c.enemies
            .enemy
            .iter()
            .map(|e| e.id.as_str())
            .collect::<Vec<_>>(),
        ENEMIES,
    );
    check(
        "news",
        &c.news.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(),
        NEWS,
    );
    check(
        "modules",
        &c.modules.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
        MODULES,
    );
    check(
        "goods",
        &c.goods.iter().map(|g| g.id.as_str()).collect::<Vec<_>>(),
        GOODS,
    );
    check(
        "convoy events",
        &c.social
            .convoy_event
            .iter()
            .map(|e| e.id.as_str())
            .collect::<Vec<_>>(),
        CONVOY_EVENTS,
    );
    check(
        "passenger kinds",
        &c.jobs
            .passenger
            .iter()
            .map(|p| p.id.as_str())
            .collect::<Vec<_>>(),
        JOB_PASSENGERS,
    );
}

/// The first sixty events and the first nine enemies and derelicts are what L2 shipped: a save
/// from then still points at the same rows.
#[test]
fn what_l2_shipped_is_where_l2_put_it() {
    let c = Content::embedded().unwrap();
    assert_eq!(c.transit.event[0].id, "dead_freighter");
    assert_eq!(c.transit.event[59].id, EVENTS[59]);
    assert_eq!(c.enemies.enemy[0].id, "skiff");
    assert_eq!(c.enemies.enemy[8].id, "sentinel");
    assert_eq!(c.derelicts.derelict[0].id, "pirate_wreck");
}
