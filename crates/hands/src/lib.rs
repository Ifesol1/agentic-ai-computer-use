//! Screens and input. Each backend (the Docker desktop, window mode on Windows,
//! macOS and Linux) implements [`Hands`]; the agent never knows which one it has.

pub mod scale;

use async_trait::async_trait;
use proto::{Action, ActionResult, Observation};

#[async_trait]
pub trait Hands: Send + Sync {
    /// Look at the window (or private desktop) this session was given.
    async fn observe(&self) -> anyhow::Result<Observation>;
    /// Do one action. `token` is from the newest observation; a stale one is refused.
    async fn act(&self, token: &str, action: &Action) -> anyhow::Result<ActionResult>;
}
