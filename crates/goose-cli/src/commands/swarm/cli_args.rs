//! The `goose swarm` sub-command argument types that need no engine context: the run options
//! struct and the pool / cloud sub-command enums. Moved verbatim from swarm.rs under the
//! incremental-split law (development_gates::swarm_rs_line_count_only_decreases), paying for the
//! `agent` sub-command's wiring (commands/swarm/agent_work).

use std::path::PathBuf;

/// Options for a `goose swarm run`.
pub struct RunOpts {
    pub prompt: String,
    pub output_format: String,
    pub log_file: Option<PathBuf>,
    pub no_log: bool,
    pub max_turns: Option<u32>,
    pub mcp: Vec<String>,
    pub research: Option<bool>,
    pub best_of_n: Option<usize>,
}

#[derive(clap::Subcommand, Debug)]
pub enum PoolCommand {
    /// Print the current pool.
    Show,
    /// Add a device.
    Add {
        id: String,
        model_id: String,
        #[arg(default_value_t = 1)]
        weight: u32,
        #[arg(default_value_t = 1)]
        instances: u32,
    },
    /// Remove a device by id.
    Rm { id: String },
    /// Set a device's weight.
    Weight { id: String, weight: u32 },
    /// Enable a device.
    Enable { id: String },
    /// Disable a device.
    Disable { id: String },
    /// Probe the live fleet (lms ps + the endpoint's model ids).
    Probe,
    /// Import every model loaded across the fleet (parses `lms ps`) as pool entries.
    Import {
        #[arg(long, default_value_t = 1)]
        weight: u32,
        #[arg(long)]
        disabled: bool,
    },
}

#[derive(clap::Subcommand, Debug)]
pub enum CloudCommand {
    /// Validate a Bedrock API key against the region, store it (AWS_BEARER_TOKEN_BEDROCK secret +
    /// AWS_REGION) ONLY if it is good, then print the auto-populated model roster.
    Key {
        key: String,
        /// AWS region the key targets (default: the stored/env AWS_REGION, else us-east-1).
        #[arg(long)]
        region: Option<String>,
        /// Machine-readable output: {"region","models":[...]} on stdout (for the desktop app).
        #[arg(long)]
        json: bool,
    },
    /// Re-validate the stored/env key and print the usable model ids (the auto-populated roster).
    Models {
        /// Machine-readable output: {"region","models":[...],"devices":[...]} on stdout.
        #[arg(long)]
        json: bool,
    },
    /// Add a Bedrock model as a swarm device (checked against the live roster first).
    Add {
        model_id: String,
        /// Concurrent tasks this cloud node may run at once.
        #[arg(long, default_value_t = 2)]
        weight: u32,
    },
    /// Remove a Bedrock swarm device by model id.
    Rm { model_id: String },
}
