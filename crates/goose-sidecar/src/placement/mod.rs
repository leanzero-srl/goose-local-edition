//! The placement planner: given a model and the Macs goose can reach, which way of running it is
//! best for what the owner wants (fastest chat, long documents, many requests) — deterministic,
//! from each Mac's measured facts, the model's own files and the speeds goose has measured.
//! Design: local-edition/mlx/DESIGN-PLACEMENT.md (approved 2026-09-24).

pub mod chip;
pub mod model;
pub mod predict;
pub mod store;
