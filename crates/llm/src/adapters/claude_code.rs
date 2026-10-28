//! A member's own Claude plan, through headless Claude Code (`claude -p --input-format stream-json --output-format stream-json`), its own tools off. Personal use only.
//!
//! Not built yet: replace the stub with the real translation. Take tool types,
//! beta headers and model ids from the provider's docs, never from memory.

use crate::{Adapter, stub::NotBuilt};

pub const ADAPTER: Adapter = Adapter {
    kind: "claude_code",
    build: |config| NotBuilt::boxed("claude_code", config),
};
