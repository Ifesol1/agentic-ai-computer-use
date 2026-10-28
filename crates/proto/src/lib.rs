//! The contract between the app, the server, the agent loop and the devices.
//!
//! Every type here travels as JSON (serde). The backend sends these types and the
//! app (`crates/app`, through `crates/client`) reads the very same Rust types, so a
//! change here is checked by the compiler on both sides at once.
//!
//! Changing a type here changes the API: see CONTRIBUTING.md, "Changing the contract".

mod computer;
mod run;

pub use computer::*;
pub use run::*;

/// Bump on every breaking change to a type in this crate.
pub const CONTRACT_VERSION: u32 = 0;
