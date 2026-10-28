//! Gemini's `generateContent`. Its computer-use model answers on a 0-999 grid: map it with `hands::scale::grid_to_screen`.
//!
//! Not built yet: replace the stub with the real translation. Take tool types,
//! beta headers and model ids from the provider's docs, never from memory.

use crate::{Adapter, stub::NotBuilt};

pub const ADAPTER: Adapter = Adapter {
    kind: "gemini",
    build: |config| NotBuilt::boxed("gemini", config),
};
