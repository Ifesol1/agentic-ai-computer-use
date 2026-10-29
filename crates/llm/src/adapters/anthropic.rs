//! Claude, through Anthropic's Messages API (`POST /v1/messages`), with its native computer tool.
//!
//! Not built yet: replace the stub with the real translation. Take tool types,
//! beta headers and model ids from the provider's docs, never from memory.

use crate::{Adapter, stub::NotBuilt};

pub const ADAPTER: Adapter = Adapter {
    kind: "anthropic",
    build: |config| NotBuilt::boxed("anthropic", config),
};
