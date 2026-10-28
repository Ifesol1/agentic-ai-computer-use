//! The neutral conversation the agent loop speaks. Each adapter in `crates/llm`
//! translates these to its provider's API and back, so the loop never knows
//! which provider it talks to.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    Image {
        /// e.g. `image/png`.
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
        content: Vec<ContentBlock>,
        #[serde(default)]
        is_error: bool,
    },
}

impl ContentBlock {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: Vec<ContentBlock>,
}

impl Message {
    pub fn user(content: Vec<ContentBlock>) -> Self {
        Self {
            role: Role::User,
            content,
        }
    }

    pub fn assistant(content: Vec<ContentBlock>) -> Self {
        Self {
            role: Role::Assistant,
            content,
        }
    }
}

/// A tool the model may call, with a JSON Schema for its arguments.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub schema: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_blocks_are_tagged_json() {
        let call = ContentBlock::ToolCall {
            id: "c1".into(),
            name: "computer".into(),
            args: serde_json::json!({"do": "observe"}),
        };
        let json = serde_json::to_value(&call).unwrap();
        assert_eq!(json["type"], "tool_call");
        assert_eq!(serde_json::from_value::<ContentBlock>(json).unwrap(), call);
    }
}
