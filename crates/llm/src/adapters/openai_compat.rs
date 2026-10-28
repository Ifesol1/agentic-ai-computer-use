//! Anything with an OpenAI-style chat API, picked by `base_url`: OpenRouter, Groq, Together, Mistral, xAI, Ollama, LM Studio, vLLM.
//!
//! Not built yet: replace the stub with the real translation. Take tool types,
//! beta headers and model ids from the provider's docs, never from memory.

use crate::{Adapter, stub::NotBuilt};

pub const ADAPTER: Adapter = Adapter {
    kind: "openai_compat",
    build: |config| NotBuilt::boxed("openai_compat", config),
};
