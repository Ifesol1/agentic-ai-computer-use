//! The contract between the frontend, the server, the agent loop and the devices.
//!
//! Every type here travels as JSON (serde). Every type marked `#[ts(export)]` is
//! also written to `web/src/gen/` as TypeScript when `cargo test` runs, so the
//! frontend imports the same shapes the server sends.
//!
//! Changing a type here changes the API: see CONTRIBUTING.md, "Changing the contract".
//! Use `u32` or `f64` for numbers, never `u64`/`i64`: ts-rs turns those into `bigint`,
//! which `JSON.parse` never produces.

mod computer;
mod run;

pub use computer::*;
pub use run::*;

/// Bump on every breaking change to a type in this crate.
pub const CONTRACT_VERSION: u32 = 0;
