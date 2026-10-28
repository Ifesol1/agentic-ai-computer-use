//! OpenAI's Responses API (`POST /v1/responses`): native computer tool where the model has one, else the generic tools.
//!
//! Not built yet: replace the stub with the real translation. Take tool types,
//! beta headers and model ids from the provider's docs, never from memory.

use crate::{Adapter, stub::NotBuilt};

pub const ADAPTER: Adapter = Adapter {
    kind: "openai",
    build: |config| NotBuilt::boxed("openai", config),
};
