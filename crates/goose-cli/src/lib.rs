// The `levers_resolved` manifest in commands/swarm.rs is one large `serde_json::json!` object whose
// key count expands `json_internal!` past the default 128-deep macro recursion limit.
#![recursion_limit = "256"]

#[cfg(not(any(feature = "rustls-tls", feature = "native-tls")))]
compile_error!("At least one of `rustls-tls` or `native-tls` features must be enabled");

#[cfg(all(feature = "rustls-tls", feature = "native-tls"))]
compile_error!("Features `rustls-tls` and `native-tls` are mutually exclusive");

pub mod cli;
pub mod commands;
pub mod edition;
pub mod logging;
pub mod project_tracker;
pub mod recipes;
pub mod scenario_tests;
pub mod session;
pub mod signal;
pub mod theme;

// Re-export commonly used types
pub use cli::Cli;
pub use session::CliSession;
