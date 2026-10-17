//! The HTTP + WebSocket API the frontend talks to.
//!
//! The routes live here once, behind the [`Backend`] trait, so `harness serve` on a
//! laptop, the future `hub` on the club server, and [`MockBackend`] all serve exactly
//! the same API. The tests in `tests/contract.rs` pin that API down.

mod mock;
mod routes;

pub use mock::MockBackend;
pub use routes::router;

use async_trait::async_trait;
use proto::{ProviderInfo, RunEvent, RunInfo, StartRun};
use tokio::sync::broadcast;

/// A request the backend won't take; sent to the client as a 400 with this message.
#[derive(Debug, Clone, PartialEq)]
pub struct BadRequest(pub String);

/// Everything the routes need from whoever runs the agents.
#[async_trait]
pub trait Backend: Send + Sync + 'static {
    /// Shown in `GET /api/health`.
    fn is_mock(&self) -> bool {
        false
    }
    async fn providers(&self) -> Vec<ProviderInfo>;
    /// Newest first.
    async fn list_runs(&self) -> Vec<RunInfo>;
    async fn get_run(&self, id: &str) -> Option<RunInfo>;
    async fn start_run(&self, req: StartRun) -> Result<RunInfo, BadRequest>;
    /// `None` if there's no such run. Stopping a finished run changes nothing.
    async fn stop_run(&self, id: &str) -> Option<RunInfo>;
    /// Every event so far, plus a receiver for the ones still to come. Both are
    /// taken together, so nothing is missed or sent twice. The receiver closes
    /// when the run finishes.
    async fn subscribe(&self, id: &str) -> Option<(Vec<RunEvent>, broadcast::Receiver<RunEvent>)>;
    /// The screenshot taken after a step: (content type, bytes).
    async fn screenshot(&self, id: &str, step: u32) -> Option<(&'static str, Vec<u8>)>;
}
