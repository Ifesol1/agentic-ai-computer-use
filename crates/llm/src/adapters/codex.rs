//! A member's own ChatGPT plan, through the official Codex CLI (`codex app-server`) in a private `CODEX_HOME`, its own tools off.
//!
//! Not built yet: replace the stub with the real translation. Take tool types,
//! beta headers and model ids from the provider's docs, never from memory.

use crate::{Adapter, stub::NotBuilt};

pub const ADAPTER: Adapter = Adapter {
    kind: "codex",
    build: |config| NotBuilt::boxed("codex", config),
};
