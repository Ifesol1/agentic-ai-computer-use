//! Model providers behind one trait. Each adapter (anthropic, openai, openai_compat,
//! gemini, the plan CLIs) translates these neutral types to its API and back, so a
//! new provider is one new file.

mod mock;

pub use mock::MockProvider;

use async_trait::async_trait;
use proto::Usage;

/// What a model can do. Set explicitly per model in config, never guessed from its name.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Caps {
    pub vision: bool,
    pub tools: bool,
    pub native_computer: bool,
    /// Longest image edge the provider takes, in pixels.
    pub max_image_edge: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Content {
    Text(String),
    Image {
        media_type: String,
        base64: String,
    },
    ToolCall {
        id: String,
        name: String,
        args: serde_json::Value,
    },
    /// Always sent right after its call: the APIs reject a call with no result.
    ToolResult {
        id: String,
        content: Vec<Content>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub role: Role,
    pub content: Vec<Content>,
}

/// A tool the model may call, with a JSON Schema for its arguments.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub schema: serde_json::Value,
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
    pub content: Vec<Content>,
    pub usage: Usage,
}

#[async_trait]
pub trait Provider: Send + Sync {
    fn caps(&self, model: &str) -> Caps;
    /// One model turn: neutral request in, neutral response out.
    async fn turn(&self, req: TurnRequest) -> anyhow::Result<TurnResponse>;
}
