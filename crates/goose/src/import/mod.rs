//! Import configuration/setup from other agents into goose.
//!
//! Unlike `goose session import` (which ingests other agents' *conversation transcripts*), this module
//! imports the **setup surface** — skills, memory, hints, MCP servers, and settings — so a user can bring
//! their existing tooling into goose. The first (and currently only) source is Claude Code.

pub mod claude_code;
