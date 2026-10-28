//! Model providers behind one trait. Each adapter (anthropic, openai, openai_compat,
//! gemini, the plan CLIs) translates the neutral types in `proto` to its API and
//! back, so the agent loop never knows which provider it talks to.
//!
//! A new provider is one new file in `src/adapters/` plus its two lines in
//! `src/adapters/mod.rs`; nothing else changes.

mod adapters;
mod mock;
mod stub;

pub use mock::MockProvider;
pub use proto::{ContentBlock, Message, Role, ToolSpec, Usage};

use async_trait::async_trait;
use serde::Deserialize;

/// What a model can do. Set explicitly per model in config, never guessed from its name.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Caps {
    pub vision: bool,
    pub tools: bool,
    pub native_computer: bool,
    /// Longest image edge the provider takes, in pixels.
    pub max_image_edge: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TurnRequest {
    pub model: String,
    pub system: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolSpec>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TurnResponse {
    pub content: Vec<ContentBlock>,
    pub usage: Usage,
}

#[async_trait]
pub trait Provider: Send + Sync {
    fn caps(&self, model: &str) -> Caps;
    /// One model turn: neutral request in, neutral response out.
    async fn turn(&self, req: TurnRequest) -> anyhow::Result<TurnResponse>;
}

/// One `[providers.<name>]` table of `harness.toml`. Never holds a key itself:
/// `api_key_env` names the variable (or keychain entry) to read it from.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct ProviderConfig {
    #[serde(default)]
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_key_env: Option<String>,
    /// Screenshots leave the PC, so computer use is opt-in per connection.
    #[serde(default)]
    pub allow_computer: bool,
}

/// How an adapter is found by its `kind =`.
pub struct Adapter {
    pub kind: &'static str,
    pub build: fn(&ProviderConfig) -> anyhow::Result<Box<dyn Provider>>,
}

/// Every adapter kind there is, e.g. for `harness providers`.
pub fn kinds() -> impl Iterator<Item = &'static str> {
    adapters::ALL.iter().map(|adapter| adapter.kind)
}

/// The provider for one config table.
pub fn build(config: &ProviderConfig) -> anyhow::Result<Box<dyn Provider>> {
    let adapter = adapters::ALL
        .iter()
        .find(|adapter| adapter.kind == config.kind)
        .ok_or_else(|| {
            let known: Vec<_> = kinds().collect();
            anyhow::anyhow!(
                "no adapter kind {:?}; known kinds: {}",
                config.kind,
                known.join(", ")
            )
        })?;
    (adapter.build)(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_are_unique() {
        let mut kinds: Vec<_> = kinds().collect();
        let count = kinds.len();
        kinds.sort();
        kinds.dedup();
        assert_eq!(kinds.len(), count, "two adapters share a kind");
    }

    #[test]
    fn unknown_kind_is_an_error() {
        let config = ProviderConfig {
            kind: "nope".into(),
            ..Default::default()
        };
        let error = build(&config).err().unwrap().to_string();
        assert!(error.contains("anthropic"), "{error}");
    }

    #[tokio::test]
    async fn every_adapter_builds() {
        for kind in kinds() {
            let config = ProviderConfig {
                kind: kind.into(),
                model: "m".into(),
                ..Default::default()
            };
            assert!(build(&config).is_ok(), "{kind}");
        }
    }
}
