//! Small pieces of atmosphere drawn from block characters: the room vignettes of the dungeon
//! view and event scenes, the skyline of Cindermere, a temple flame. All are a pure function of
//! their inputs (the "phase" only changes when the hero acts), so a snapshot and a replay draw the
//! same picture. In the ASCII tier the framework turns blocks into `#`, `:` and `.`; the pieces
//! here pick plain letters where that would read badly.

use russh_bbs_door::buffer::Buffer;
use russh_bbs_door::layout::Rect;
use russh_bbs_door::style::{Color, Style};
use russh_bbs_door::widgets::Widget;

use super::kit::{
    AMBER, BLOOD, BONE, COAL, DEEP, DEEP_BRIGHT, EMBER_DIM, EMBER_HOT, Look, STONE, STONE_DARK,
    STONE_DEEP, hash, heat, lerp,
};
use crate::content::RoomKind;

/// A short picture for a room, as wide as the area and as tall as it (up to a few rows).
pub(super) struct Vignette {
    pub kind: RoomKind,
    pub look: Look,
    /// Draws the room as one that is lit (a torch or a fire here).
    pub warmth: f32,
    /// A seed that changes from room to room.
    pub seed: u32,
}

impl Widget for Vignette {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 8 || area.height == 0 {
            return;
        }
        let w = u32::from(area.width);
        let ascii = self.look.ascii;
        for row in 0..area.height {
            for col in 0..area.width {
                let h = hash(
                    u32::from(col),
                    u32::from(row),
                    self.seed ^ (self.look.phase / 5),
                );
                let (sym, color): (&str, Color) = match self.kind {
                    RoomKind::Forge => {
                        // Rising sparks over a glowing bed of coals at the bottom.
                        let from_bottom = area.height - 1 - row;
                        if from_bottom == 0 {
                            (
                                if ascii { "=" } else { "▀" },
                                heat(0.35 + 0.5 * self.warmth * f32::from((h % 5) as u8) / 4.0),
                            )
                        } else if h.is_multiple_of(6 + u32::from(from_bottom) * 4) {
                            (
                                if ascii { "'" } else { "·" },
                                heat(0.9 - 0.2 * f32::from(from_bottom)),
                            )
                        } else {
                            (" ", COAL)
                        }
                    }
                    RoomKind::Flooded => {
                        let wave = (u32::from(col) + u32::from(row) * 3 + self.look.phase) % 6;
                        if row == 0 && wave < 3 {
                            (if ascii { "~" } else { "≈" }, DEEP_BRIGHT)
                        } else if row > 0 && wave < 2 {
                            ("~", lerp(DEEP, DEEP_BRIGHT, f32::from(row) / 4.0))
                        } else {
                            (" ", DEEP)
                        }
                    }
                    RoomKind::Shrine => {
                        // Candles along the bottom, each with a small flame.
                        let from_bottom = area.height - 1 - row;
                        let candle = (u32::from(col) + self.seed) % 7 == 3;
                        if candle && from_bottom == 0 {
                            ("i", BONE)
                        } else if candle && from_bottom == 1 {
                            (
                                if ascii { "^" } else { "▲" },
                                heat(0.7 + 0.3 * f32::from((h % 3) as u8) / 2.0),
                            )
                        } else {
                            (" ", STONE_DEEP)
                        }
                    }
                    RoomKind::Lair | RoomKind::Guardian => {
                        // Eyes in the dark.
                        let pair = (h.is_multiple_of(29) && col + 2 < area.width)
                            && (row + 1 < area.height);
                        if pair {
                            (if ascii { "o" } else { "◉" }, BLOOD)
                        } else if h % 23 == 1 {
                            ("·", STONE_DARK)
                        } else {
                            (" ", STONE_DEEP)
                        }
                    }
                    RoomKind::Vault => {
                        if h.is_multiple_of(13) {
                            (if ascii { "*" } else { "✦" }, EMBER_HOT)
                        } else if h.is_multiple_of(9) {
                            ("$", EMBER_DIM)
                        } else {
                            (" ", STONE_DEEP)
                        }
                    }
                    RoomKind::Shaft => {
                        let chain = u32::from(col) % 9 == 4;
                        if chain {
                            (if row % 2 == 0 { "║" } else { "│" }, STONE)
                        } else {
                            (" ", STONE_DEEP)
                        }
                    }
                    RoomKind::Exit => {
                        let step = u32::from(row) * 3 + 2;
                        if u32::from(col) >= step && u32::from(col) < step + 6 {
                            (
                                if ascii { "=" } else { "▄" },
                                lerp(STONE, STONE_DARK, f32::from(row) / 4.0),
                            )
                        } else {
                            (" ", STONE_DEEP)
                        }
                    }
                    RoomKind::Entry | RoomKind::Corridor => {
                        // Arches receding into the dark.
                        let arch = (u32::from(col) + self.seed).is_multiple_of(11);
                        if arch {
                            (
                                "│",
                                lerp(STONE, STONE_DARK, (u32::from(col) * 100 / w) as f32 / 100.0),
                            )
                        } else if h.is_multiple_of(41) {
                            ("·", STONE_DARK)
                        } else {
                            (" ", STONE_DEEP)
                        }
                    }
                };
                if sym != " " {
                    buf.set_string(area.x + col, area.y + row, sym, Style::new().fg(color));
                }
            }
        }
    }
}

/// Cindermere under its hill: a night sky, a ridge, rooftops with lit windows and the glow of the
/// forges below. Taller areas get a taller hill.
pub(super) struct Skyline {
    pub look: Look,
    /// The calendar day, so each morning's sky is its own.
    pub day: i64,
    /// How many of the town's works are finished, 0 to 11: more houses on the hill.
    pub built: u32,
    /// How far the forge glow reaches, 0 to 1.
    pub glow: f32,
}

impl Widget for Skyline {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 12 || area.height < 4 {
            return;
        }
        let day = u32::try_from(self.day.rem_euclid(1 << 20)).unwrap_or(0);
        let w = area.width;
        let h = area.height;
        let ascii = self.look.ascii || !self.look.color;
        // Stars and a moon in the upper rows.
        for y in 0..h.saturating_sub(3) {
            for x in 0..w {
                let n = hash(u32::from(x), u32::from(y), day);
                if n.is_multiple_of(29) {
                    buf.set_string(
                        area.x + x,
                        area.y + y,
                        if n.is_multiple_of(4) { "*" } else { "." },
                        Style::new().fg(if n.is_multiple_of(5) { BONE } else { STONE }),
                    );
                }
            }
        }
        buf.set_string(
            area.x + w / 6,
            area.y,
            if ascii { "C" } else { "◖" },
            Style::new().fg(BONE),
        );
        // The hill: a smooth ridge in eighths of a cell.
        let ridge = |x: u16| -> f32 {
            let t = f32::from(x) / f32::from(w.max(2) - 1);
            let big = (t * std::f32::consts::PI).sin();
            let ripple = (t * 9.0 + (day % 7) as f32).sin() * 0.06;
            ((big * 0.62 + 0.18 + ripple) * f32::from(h - 1)).clamp(1.0, f32::from(h - 1))
        };
        let partial = ["▁", "▂", "▃", "▄", "▅", "▆", "▇"];
        for x in 0..w {
            let hf = ridge(x);
            let full = hf.floor() as u16;
            for k in 0..full {
                buf.set_string(
                    area.x + x,
                    area.y + h - 1 - k,
                    if ascii { "#" } else { "█" },
                    Style::new().fg(STONE),
                );
            }
            let part = ((hf - hf.floor()) * 8.0) as usize;
            if part > 0 && full < h {
                buf.set_string(
                    area.x + x,
                    area.y + h - 1 - full,
                    if ascii { "." } else { partial[part.min(7) - 1] },
                    Style::new().fg(STONE),
                );
            }
        }
        // Houses on the hill: a red roof over a dark wall with a lit window; more as the town builds.
        let houses = 3 + self.built.min(5);
        let fx = w * 3 / 4;
        for i in 0..houses {
            let hw = 4 + (hash(i, 7, day) % 2) as u16;
            let hx = 1
                + (w.saturating_sub(hw + 2)) * u16::try_from(i).unwrap_or(0)
                    / u16::try_from(houses).unwrap_or(1);
            if hx + hw >= w || (hx + hw / 2).abs_diff(fx) < 4 {
                continue;
            }
            let top = ridge(hx + hw / 2).floor() as u16;
            if top + 2 >= h {
                continue;
            }
            let wall_y = area.y + h - 1 - top;
            let roof_y = wall_y.saturating_sub(1);
            for dx in 0..hw {
                buf.set_string(
                    area.x + hx + dx,
                    roof_y,
                    if ascii { "^" } else { "▄" },
                    Style::new().fg(EMBER_DIM),
                );
                buf.set_string(
                    area.x + hx + dx,
                    wall_y,
                    if ascii { "#" } else { "█" },
                    Style::new().fg(Color::Rgb(0, 0, 0)),
                );
            }
            if !hash(i, 3, day).is_multiple_of(3) {
                buf.set_string(
                    area.x + hx + hw / 2,
                    wall_y,
                    if ascii { "o" } else { "▪" },
                    Style::new().fg(AMBER),
                );
            }
        }
        // The forge on the right: smoke over a glowing mouth.
        let top = ridge(fx).floor() as u16;
        let foot = area.y + h - 1 - top.min(h - 1);
        for k in 0..3u16 {
            let drift = (hash(u32::from(k), 11, self.look.phase) % 3) as u16;
            if foot > area.y + k + 1 {
                buf.set_string(
                    area.x + (fx + drift).min(w - 1),
                    foot - 1 - k,
                    if ascii { "." } else { "░" },
                    Style::new().fg(STONE),
                );
            }
        }
        for dx in 0..3u16 {
            buf.set_string(
                area.x + fx + dx,
                foot,
                if ascii { "=" } else { "▀" },
                Style::new().fg(heat(0.6 + 0.35 * self.glow)),
            );
        }
    }
}

/// A flame for the temple: a small teardrop of colour that leans a little with the phase.
pub(super) fn flame_rows(look: &Look) -> [&'static str; 3] {
    let lean = look.phase.is_multiple_of(2);
    if look.ascii {
        if lean {
            [" ) ", "(%)", " # "]
        } else {
            [" ( ", "(%)", " # "]
        }
    } else if lean {
        [" ▲ ", "◢█◣", " ▀ "]
    } else {
        [" ▲ ", "◥█◤", " ▀ "]
    }
}
