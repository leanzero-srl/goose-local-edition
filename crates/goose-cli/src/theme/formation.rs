//! The swarm fan-in render unit — Goose Local Edition's signature.
//!
//! Goose's reality is N models in *formation* (tensor-parallel over Thunderbolt/JACCL, or an LM-Studio
//! swarm). This renders that as a **dispatch → braid → fan-in** unit: one dispatch header, parallel node
//! lanes, and a rolled-up result. Node identity is a SOLID inline formation-hue chip (`⬢` + letter) — an
//! inline leading token, never a left rail (a hard project UI rule). Status uses goose's own glyphs
//! (`●`/`✔`/`✕`) in the semantic triad, which is orthogonal to the identity ramp — so a red status never
//! reads as a node's identity. This is what a single-model CLI cannot show, and it is reused-not-cloned:
//! goose's own glyphs and swarm-led framing, not Claude Code's `⏺` or its exact strings.

use super::palette::{node_hue, Rgb, BOLD, DIM, ERR, OK, RESET, WARN};

/// A node's live status within the formation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeStatus {
    Running,
    Done,
    Error,
}

impl NodeStatus {
    /// Goose's own status glyph (not Claude Code's `⏺`).
    pub fn glyph(self) -> &'static str {
        match self {
            NodeStatus::Running => "●",
            NodeStatus::Done => "✔",
            NodeStatus::Error => "✕",
        }
    }

    /// The semantic-triad hue for this status (orthogonal to the node identity ramp).
    pub fn hue(self) -> Rgb {
        match self {
            NodeStatus::Running => WARN,
            NodeStatus::Done => OK,
            NodeStatus::Error => ERR,
        }
    }
}

/// One node lane in the fan-in unit.
pub struct NodeLane<'a> {
    /// The node's slot in the formation (drives its identity hue + letter).
    pub index: usize,
    /// The device name (e.g. "m4-max").
    pub device: &'a str,
    /// What this node is doing / did.
    pub action: &'a str,
    pub status: NodeStatus,
}

/// The identity letter for a node index (A, B, C, …).
fn node_letter(index: usize) -> char {
    (b'A' + (index % 26) as u8) as char
}

/// The solid formation-hue identity chip for a node — an inline leading token (`⬢A`), never a rail.
pub fn node_chip(index: usize) -> String {
    let hue = node_hue(index);
    format!("{BOLD}{}⬢{}{RESET}", hue.fg(), node_letter(index))
}

/// Render the swarm fan-in unit: a dispatch header, one lane per node (solid formation chip + device +
/// action + status glyph), and a rolled-up fan-in footer.
pub fn render_fan_in(dispatch: &str, lanes: &[NodeLane]) -> String {
    let done = lanes
        .iter()
        .filter(|l| l.status == NodeStatus::Done)
        .count();
    let mut out = String::new();
    out.push_str(&format!(
        "  swarm · {dispatch}   {}{} nodes · {done}/{} done{RESET}\n",
        DIM.fg(),
        lanes.len(),
        lanes.len(),
    ));
    for lane in lanes {
        let chip = node_chip(lane.index);
        let status = format!("{}{}{RESET}", lane.status.hue().fg(), lane.status.glyph());
        out.push_str(&format!(
            "   {chip} {:<10} {:<30} {status}\n",
            lane.device, lane.action,
        ));
    }
    out.push_str(&format!(
        "   {}▾ fan-in · {} lane(s){RESET}\n",
        DIM.fg(),
        lanes.len(),
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::palette::STATUS_TRIAD;

    #[test]
    fn fan_in_execute_phase_visual() {
        // Mirrors exactly what swarm.rs builds from report.per_device at end of the execute phase.
        let actions = [
            "7 tasks · 4210ms avg",
            "5 tasks · 5130ms avg",
            "4 tasks · 6820ms avg",
        ];
        let lanes: Vec<NodeLane> = ["m4-max", "m3-ultra", "studio-2"]
            .iter()
            .enumerate()
            .map(|(i, d)| NodeLane {
                index: i,
                device: d,
                action: actions[i],
                status: NodeStatus::Done,
            })
            .collect();
        println!("\n{}", render_fan_in("execute", &lanes));
    }

    #[test]
    fn fan_in_renders_chips_and_disjoint_hues() {
        let lanes = [
            NodeLane {
                index: 0,
                device: "m4-max",
                action: "edit auth.rs",
                status: NodeStatus::Done,
            },
            NodeLane {
                index: 1,
                device: "m3-ultra",
                action: "grep callsites",
                status: NodeStatus::Running,
            },
            NodeLane {
                index: 2,
                device: "studio-2",
                action: "cargo test",
                status: NodeStatus::Error,
            },
        ];
        let out = render_fan_in("dispatch", &lanes);

        // inline node chips present (not a left rail)
        assert!(out.contains("⬢A"));
        assert!(out.contains("⬢B"));
        assert!(out.contains("⬢C"));
        // goose glyphs, not Claude's ⏺
        assert!(out.contains('●') && out.contains('✔') && out.contains('✕'));
        assert!(!out.contains('⏺'));
        // fan-in footer
        assert!(out.contains("fan-in"));

        // each node's identity hue is disjoint from every status hue
        for i in 0..3 {
            let node_seq = node_hue(i).fg();
            for status in STATUS_TRIAD {
                assert_ne!(node_seq, status.fg(), "node hue equals a status hue");
            }
        }
    }

    #[test]
    fn node_chip_is_a_leading_inline_token_not_a_rail() {
        let chip = node_chip(0);
        // the chip is the hexagon glyph + letter, colored — no box-drawing rail chars
        assert!(chip.contains("⬢A"));
        assert!(!chip.contains('│') && !chip.contains('▌') && !chip.contains('┃'));
    }
}
