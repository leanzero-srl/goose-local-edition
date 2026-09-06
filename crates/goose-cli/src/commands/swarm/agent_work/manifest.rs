//! `agent.yaml` — the one file that turns a directory into an AGENT the swarm can run.
//!
//! An agent here is what the operator's desk skills already are on disk: a charter (the rules),
//! read-only poll scripts that produce the tick's inbox, surgeon charters the orchestrator fans
//! items to, a refuting review, ONE gated write command, and the durable files it keeps
//! (a daily log, a pending file for the human, a scratchpad). The manifest names those; the
//! engine (tick.rs) runs them. Nothing here is a cap on model work: `cadence` is when the next
//! tick STARTS, the window is when the desk is open.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const MANIFEST_FILE: &str = "agent.yaml";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentManifest {
    pub name: String,
    #[serde(default)]
    pub title: String,
    /// The charter FILE (relative to the agent dir): the desk's rules, read whole into the
    /// orchestrator's prompt. `brief` is inline text used with it or instead of it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub charter: Option<String>,
    #[serde(default)]
    pub brief: String,
    #[serde(default = "default_timezone")]
    pub timezone: String,
    #[serde(default)]
    pub window: WorkWindow,
    /// Time between tick STARTS ("30m", "2h", "90s"). A tick that overruns starts the next one
    /// as soon as it ends — never cut.
    #[serde(default = "default_cadence")]
    pub cadence: String,
    /// `KEY=VALUE` lines sourced into every script and lane (secrets stay in this file, never in
    /// the manifest or the events).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env_file: Option<String>,
    /// Shell lines run BEFORE anything else each tick. Exit 3 = HOLD this tick (the script names
    /// why on stdout); any other non-zero exit is a warning that rides the events.
    #[serde(default)]
    pub guard: Vec<String>,
    /// Read-only shell lines whose stdout IS the tick's inbox (what needs attention).
    #[serde(default)]
    pub poll: Vec<String>,
    /// Shell lines run at the end of every tick (close-out scripts, log rotation).
    #[serde(default)]
    pub close: Vec<String>,
    #[serde(default)]
    pub surgeons: Vec<Surgeon>,
    #[serde(default)]
    pub review: Review,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post: Option<PostConfig>,
    #[serde(default = "default_ledger")]
    pub ledger: String,
    #[serde(default = "default_pending")]
    pub pending: String,
    #[serde(default = "default_scratchpad")]
    pub scratchpad: String,
    /// Commit the agent directory after every tick (each desk is its own git repo).
    #[serde(default = "default_true")]
    pub commit: bool,
    /// MCP worker extensions by builder name (context7 | web-search | doc-processor), added to
    /// every lane on top of the developer tools.
    #[serde(default)]
    pub extensions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkWindow {
    /// Empty = every day.
    #[serde(default = "default_days")]
    pub days: Vec<String>,
    #[serde(default = "default_from")]
    pub from: String,
    #[serde(default = "default_to")]
    pub to: String,
    /// True = the desk never closes (the window above is ignored).
    #[serde(default)]
    pub always: bool,
}

impl Default for WorkWindow {
    fn default() -> Self {
        Self {
            days: default_days(),
            from: default_from(),
            to: default_to(),
            always: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Surgeon {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub charter: Option<String>,
    #[serde(default)]
    pub brief: String,
    /// Words the orchestrator may use to route an item here; advisory — the orchestrator decides.
    #[serde(default, rename = "match")]
    pub match_words: Vec<String>,
    /// A read-only surgeon gets no file-writing tools; it returns a vetted draft, it never posts.
    #[serde(default = "default_true")]
    pub read_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Review {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Adversarial lenses, one lane each, every one told to REFUTE the draft.
    #[serde(default = "default_lenses")]
    pub lenses: Vec<String>,
}

impl Default for Review {
    fn default() -> Self {
        Self {
            enabled: true,
            lenses: default_lenses(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostConfig {
    /// The ONE write path. `{id}` is the staged draft's id, `{file}` the path of its body on disk.
    pub command: String,
    #[serde(default)]
    pub approval: Approval,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Approval {
    /// A staged draft posts only after the human approves it in the desk.
    #[default]
    Human,
    /// A staged draft that survived review posts on the NEXT tick (the two-tick spine still holds).
    None,
}

fn default_timezone() -> String {
    "Europe/Bucharest".to_string()
}
fn default_cadence() -> String {
    "30m".to_string()
}
fn default_days() -> Vec<String> {
    ["mon", "tue", "wed", "thu", "fri"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}
fn default_from() -> String {
    "09:00".to_string()
}
fn default_to() -> String {
    "18:00".to_string()
}
fn default_ledger() -> String {
    "DAILY-LOG.md".to_string()
}
fn default_pending() -> String {
    "PENDING.md".to_string()
}
fn default_scratchpad() -> String {
    "SCRATCHPAD.md".to_string()
}
fn default_true() -> bool {
    true
}
fn default_lenses() -> Vec<String> {
    ["factual", "duplication", "voice"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

impl AgentManifest {
    pub fn path_in(dir: &Path) -> PathBuf {
        dir.join(MANIFEST_FILE)
    }

    pub fn load(dir: &Path) -> Result<Self> {
        let path = Self::path_in(dir);
        let text = std::fs::read_to_string(&path)
            .map_err(|e| anyhow!("reading {}: {e}", path.display()))?;
        let m: Self =
            serde_yaml::from_str(&text).map_err(|e| anyhow!("parsing {}: {e}", path.display()))?;
        m.validate()?;
        Ok(m)
    }

    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(anyhow!("agent.yaml: `name` is required"));
        }
        if !self
            .name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(anyhow!(
                "agent.yaml: `name` must be [A-Za-z0-9_-] (it names files and lane keys)"
            ));
        }
        super::window::parse_cadence(&self.cadence)
            .ok_or_else(|| anyhow!("agent.yaml: cadence `{}` is not <n>s|m|h", self.cadence))?;
        self.timezone.parse::<chrono_tz::Tz>().map_err(|_| {
            anyhow!(
                "agent.yaml: timezone `{}` is not an IANA zone",
                self.timezone
            )
        })?;
        super::window::parse_hm(&self.window.from).ok_or_else(|| {
            anyhow!(
                "agent.yaml: window.from `{}` is not HH:MM",
                self.window.from
            )
        })?;
        super::window::parse_hm(&self.window.to)
            .ok_or_else(|| anyhow!("agent.yaml: window.to `{}` is not HH:MM", self.window.to))?;
        for d in &self.window.days {
            super::window::parse_day(d)
                .ok_or_else(|| anyhow!("agent.yaml: window.days entry `{d}` is not mon..sun"))?;
        }
        let mut seen = std::collections::HashSet::new();
        for s in &self.surgeons {
            if s.name.trim().is_empty() {
                return Err(anyhow!("agent.yaml: a surgeon has no name"));
            }
            if !seen.insert(s.name.clone()) {
                return Err(anyhow!(
                    "agent.yaml: surgeon `{}` is declared twice",
                    s.name
                ));
            }
        }
        if let Some(p) = &self.post {
            if p.command.trim().is_empty() {
                return Err(anyhow!("agent.yaml: post.command is empty"));
            }
        }
        Ok(())
    }

    pub fn display_title(&self) -> &str {
        if self.title.trim().is_empty() {
            &self.name
        } else {
            &self.title
        }
    }

    /// The charter as the orchestrator reads it: the file (whole) then the inline brief. An
    /// absent file is a NAMED absence in the returned text, never a silent blank.
    pub fn charter_text(&self, dir: &Path) -> String {
        let mut out = String::new();
        if let Some(rel) = &self.charter {
            let p = dir.join(rel);
            match std::fs::read_to_string(&p) {
                Ok(t) => out.push_str(t.trim_end()),
                Err(e) => out.push_str(&format!(
                    "(charter file {} could not be read: {e})",
                    p.display()
                )),
            }
        }
        if !self.brief.trim().is_empty() {
            if !out.is_empty() {
                out.push_str("\n\n");
            }
            out.push_str(self.brief.trim());
        }
        if out.is_empty() {
            out.push_str("(no charter: agent.yaml names neither `charter` nor `brief`)");
        }
        out
    }

    pub fn surgeon(&self, name: &str) -> Option<&Surgeon> {
        self.surgeons.iter().find(|s| s.name == name)
    }

    /// A starter manifest for `goose swarm agent init`.
    pub fn starter(name: &str) -> String {
        format!(
            "name: {name}\ntitle: {name} desk\n# The desk's rules — a file read whole into the orchestrator's prompt, and/or inline text.\ncharter: CHARTER.md\nbrief: \"\"\ntimezone: Europe/Bucharest\nwindow:\n  days: [mon, tue, wed, thu, fri]\n  from: \"09:00\"\n  to: \"18:00\"\n  always: false\n# Time between tick starts. A tick is never cut; an overrunning tick starts the next when it ends.\ncadence: 30m\n# KEY=VALUE lines sourced into every script and lane.\n# env_file: references/credentials.env\n# Exit 3 from a guard = hold this tick (say why on stdout).\nguard: []\n# Read-only commands whose stdout is the tick's inbox.\npoll:\n  - \"echo 'nothing polled yet: put a read-only triage command here'\"\nclose: []\nsurgeons:\n  - name: general\n    brief: |\n      You handle one item at a time. Do the homework with read-only commands, then hand back a\n      HOMEWORK / FINDING / DRAFT / ASK block with exact identifiers and one concrete next step.\n    match: []\n    read_only: true\nreview:\n  enabled: true\n  lenses: [factual, duplication, voice]\n# The ONE write path. {{id}} = staged draft id, {{file}} = its body on disk.\n# post:\n#   command: \"python3 scripts/post.py --prepared {{id}}\"\n#   approval: human\nledger: DAILY-LOG.md\npending: PENDING.md\nscratchpad: SCRATCHPAD.md\ncommit: true\nextensions: []\n"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starter_manifest_parses_and_validates() {
        let m: AgentManifest = serde_yaml::from_str(&AgentManifest::starter("axpo")).unwrap();
        m.validate().unwrap();
        assert_eq!(m.name, "axpo");
        assert_eq!(m.cadence, "30m");
        assert_eq!(m.surgeons.len(), 1);
        assert!(m.post.is_none());
        assert_eq!(m.review.lenses, vec!["factual", "duplication", "voice"]);
    }

    #[test]
    fn a_bad_cadence_or_zone_is_refused_by_name() {
        let mut m: AgentManifest = serde_yaml::from_str(&AgentManifest::starter("x")).unwrap();
        m.cadence = "soon".into();
        assert!(m.validate().unwrap_err().to_string().contains("cadence"));
        m.cadence = "5m".into();
        m.timezone = "Mars/Olympus".into();
        assert!(m.validate().unwrap_err().to_string().contains("timezone"));
    }

    #[test]
    fn charter_text_names_a_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut m: AgentManifest = serde_yaml::from_str(&AgentManifest::starter("x")).unwrap();
        m.charter = Some("nope.md".into());
        m.brief = "be terse".into();
        let t = m.charter_text(dir.path());
        assert!(t.contains("could not be read"));
        assert!(t.ends_with("be terse"));
    }
}
