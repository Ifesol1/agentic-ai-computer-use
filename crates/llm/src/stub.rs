//! Stands in for an adapter that isn't written yet, so config, `harness providers`
//! and the tests already know its kind.

use anyhow::bail;
use async_trait::async_trait;

use crate::{Caps, Provider, ProviderConfig, TurnRequest, TurnResponse};

pub struct NotBuilt {
    kind: &'static str,
}

impl NotBuilt {
    pub fn boxed(
        kind: &'static str,
        _config: &ProviderConfig,
    ) -> anyhow::Result<Box<dyn Provider>> {
        Ok(Box::new(Self { kind }))
    }
}

#[async_trait]
impl Provider for NotBuilt {
    fn caps(&self, _model: &str) -> Caps {
        Caps::default()
    }

    async fn turn(&self, _req: TurnRequest) -> anyhow::Result<TurnResponse> {
        bail!(
            "the {} adapter isn't built yet (crates/llm/src/adapters)",
            self.kind
        )
    }
}
