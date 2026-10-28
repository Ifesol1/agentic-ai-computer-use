use std::{collections::VecDeque, sync::Mutex};

use anyhow::anyhow;
use async_trait::async_trait;
use proto::{ContentBlock, Usage};

use crate::{Caps, Provider, TurnRequest, TurnResponse};

/// Replays scripted replies in order: tests the loop in CI with no keys and no cost.
pub struct MockProvider {
    replies: Mutex<VecDeque<Vec<ContentBlock>>>,
    /// Every request it got, for tests to check what the loop sent.
    requests: Mutex<Vec<TurnRequest>>,
}

impl MockProvider {
    pub fn new(replies: impl IntoIterator<Item = Vec<ContentBlock>>) -> Self {
        Self {
            replies: Mutex::new(replies.into_iter().collect()),
            requests: Mutex::default(),
        }
    }

    /// One scripted reply: an optional note, then one tool call.
    pub fn call(note: &str, id: &str, tool: &str, args: serde_json::Value) -> Vec<ContentBlock> {
        let mut reply = Vec::new();
        if !note.is_empty() {
            reply.push(ContentBlock::text(note));
        }
        reply.push(ContentBlock::ToolCall {
            id: id.into(),
            name: tool.into(),
            args,
        });
        reply
    }

    pub fn requests(&self) -> Vec<TurnRequest> {
        self.requests.lock().unwrap().clone()
    }
}

#[async_trait]
impl Provider for MockProvider {
    fn caps(&self, _model: &str) -> Caps {
        Caps {
            vision: true,
            tools: true,
            native_computer: false,
            max_image_edge: 1280,
        }
    }

    async fn turn(&self, req: TurnRequest) -> anyhow::Result<TurnResponse> {
        self.requests.lock().unwrap().push(req);
        let content = self
            .replies
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| anyhow!("the mock provider ran out of scripted replies"))?;
        Ok(TurnResponse {
            content,
            // Plausible numbers, so budgets and cost logs can be tested too.
            usage: Usage {
                input_tokens: 1_000,
                output_tokens: 50,
                cost_usd: 0.001,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> TurnRequest {
        TurnRequest {
            model: "scripted".into(),
            system: String::new(),
            messages: Vec::new(),
            tools: Vec::new(),
        }
    }

    #[tokio::test]
    async fn replies_in_order_then_errors() {
        let mock = MockProvider::new([
            vec![ContentBlock::text("one")],
            vec![ContentBlock::text("two")],
        ]);
        assert_eq!(
            mock.turn(request()).await.unwrap().content,
            [ContentBlock::text("one")]
        );
        assert_eq!(
            mock.turn(request()).await.unwrap().content,
            [ContentBlock::text("two")]
        );
        assert!(mock.turn(request()).await.is_err());
        assert_eq!(mock.requests().len(), 3);
    }
}
