//! A member's own Google AI plan, through a headless Antigravity (`agy`) run. Text only: it needs the accessibility outline.
//!
//! Not built yet: replace the stub with the real translation. Take tool types,
//! beta headers and model ids from the provider's docs, never from memory.

use crate::{Adapter, stub::NotBuilt};

pub const ADAPTER: Adapter = Adapter {
    kind: "antigravity",
    build: |config| NotBuilt::boxed("antigravity", config),
};
