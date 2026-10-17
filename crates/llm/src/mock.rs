use std::{collections::VecDeque, sync::Mutex};

use anyhow::anyhow;
use async_trait::async_trait;
use proto::Usage;

use crate::{Caps, Content, Provider, TurnRequest, TurnResponse};

/// Replays scripted replies in order: tests the loop in CI with no keys and no cost.
pub struct MockProvider {
    replies: Mutex<VecDeque<Vec<Content>>>,
}

impl MockProvider {
    pub fn new(replies: impl IntoIterator<Item = Vec<Content>>) -> Self {
        Self {
            replies: Mutex::new(replies.into_iter().collect()),
        }
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

    async fn turn(&self, _req: TurnRequest) -> anyhow::Result<TurnResponse> {
        let content = self
            .replies
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| anyhow!("the mock provider ran out of scripted replies"))?;
        Ok(TurnResponse {
            content,
            usage: Usage::default(),
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
            vec![Content::Text("one".into())],
            vec![Content::Text("two".into())],
        ]);
        assert_eq!(
            mock.turn(request()).await.unwrap().content,
            [Content::Text("one".into())]
        );
        assert_eq!(
            mock.turn(request()).await.unwrap().content,
            [Content::Text("two".into())]
        );
        assert!(mock.turn(request()).await.is_err());
    }
}
