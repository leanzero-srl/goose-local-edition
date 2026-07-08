//! Goose Local Edition color palette — two orthogonal axes.
//!
//! Axis 1 (node identity): a 6-hue "formation ramp" (cyan → rose), all high-saturation, reinforcing the
//! parallel-swarm metaphor by hue position — the mesh lighting up is the thing a single-model CLI can't
//! show. Axis 2 (semantic status): OK / WARN / ERR / DIM. The two axes are DISJOINT — no node hue equals a
//! status hue — so a node's identity chip is never confusable with its status. All colors are 24-bit
//! (truecolor) ANSI. Solid, saturated fills only — no faded tints (a hard project UI rule).

/// An RGB color renderable as a truecolor ANSI foreground/background.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// The ANSI truecolor foreground SGR sequence.
    pub fn fg(self) -> String {
        format!("\x1b[38;2;{};{};{}m", self.0, self.1, self.2)
    }

    /// The ANSI truecolor background SGR sequence.
    pub fn bg(self) -> String {
        format!("\x1b[48;2;{};{};{}m", self.0, self.1, self.2)
    }

    /// Wrap `text` in this foreground color and reset.
    pub fn paint(self, text: &str) -> String {
        format!("{}{text}{RESET}", self.fg())
    }

    /// Wrap `text` in this foreground color, bold, and reset.
    pub fn paint_bold(self, text: &str) -> String {
        format!("{BOLD}{}{text}{RESET}", self.fg())
    }
}

pub const RESET: &str = "\x1b[0m";
pub const BOLD: &str = "\x1b[1m";

/// The 6-hue formation ramp for node identity (cyan → rose), disjoint from the status triad.
pub const FORMATION_RAMP: [Rgb; 6] = [
    Rgb(0x17, 0xc4, 0xc4), // cyan-teal
    Rgb(0x2e, 0x8b, 0xff), // azure — also the signature accent
    Rgb(0x6a, 0x5c, 0xff), // indigo
    Rgb(0xb1, 0x4c, 0xff), // violet
    Rgb(0xff, 0x3e, 0xa5), // magenta
    Rgb(0xff, 0x5c, 0x7a), // rose
];

/// The signature Local-Edition accent (the ramp's anchor — a migratory-dusk azure, goose's own, not a
/// borrowed corporate teal). Used for the LOCAL lockup and live emphasis.
pub const ACCENT: Rgb = FORMATION_RAMP[1];

/// Semantic status triad + a dim neutral — orthogonal to the formation ramp.
pub const OK: Rgb = Rgb(0x2e, 0xcc, 0x71);
pub const WARN: Rgb = Rgb(0xf5, 0xa6, 0x23);
pub const ERR: Rgb = Rgb(0xff, 0x3b, 0x30);
pub const DIM: Rgb = Rgb(0x87, 0x87, 0x87);

/// The full status set, for disjointness checks.
pub const STATUS_TRIAD: [Rgb; 4] = [OK, WARN, ERR, DIM];

/// The formation hue for node index `i` (wraps around the ramp).
pub fn node_hue(i: usize) -> Rgb {
    FORMATION_RAMP[i % FORMATION_RAMP.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_ramp_is_disjoint_from_status_triad() {
        // A node identity hue must never equal a status hue, or a chip reads as a status.
        for node in FORMATION_RAMP {
            for status in STATUS_TRIAD {
                assert_ne!(node, status, "formation hue collides with a status hue");
            }
        }
    }

    #[test]
    fn node_hue_wraps() {
        assert_eq!(node_hue(0), FORMATION_RAMP[0]);
        assert_eq!(node_hue(6), FORMATION_RAMP[0]);
        assert_eq!(node_hue(7), FORMATION_RAMP[1]);
    }

    #[test]
    fn truecolor_sequences_are_well_formed() {
        assert_eq!(Rgb(0x2e, 0x8b, 0xff).fg(), "\x1b[38;2;46;139;255m");
        assert!(ACCENT.paint("x").ends_with(RESET));
    }
}
