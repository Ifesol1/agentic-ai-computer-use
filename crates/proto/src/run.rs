//! Runs: what the frontend starts, lists and watches.

use serde::{Deserialize, Serialize};

use crate::Action;

/// `POST /api/runs` body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartRun {
    /// What the person asked for, in their words.
    pub prompt: String,
    /// A provider name from `GET /api/providers`; left out = the default one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Overrides the provider's default model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Where the agent works; left out = a private desktop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<Target>,
    /// Stop after this many steps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_steps: Option<u32>,
    /// Stop when the run has cost this much.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cost_usd: Option<f64>,
}

/// Where a run's agent works.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Target {
    /// A virtual desktop (Docker + Xvfb) that nobody else uses.
    #[default]
    PrivateDesktop,
    /// One window on a member's PC. The person picks it, never the model.
    Window { device: String, window: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Queued,
    Running,
    /// Done, and an observation shows the result (not just "the input was sent").
    Succeeded,
    Failed,
    /// Stopped by a person or a budget.
    Stopped,
}

impl RunStatus {
    pub fn is_final(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Stopped)
    }
}

/// Running totals for one run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cost_usd: f64,
}

/// A run as `GET /api/runs` and `GET /api/runs/{id}` return it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunInfo {
    pub id: String,
    pub prompt: String,
    pub provider: String,
    pub model: String,
    pub target: Target,
    pub status: RunStatus,
    /// RFC 3339, UTC.
    pub created_at: String,
    /// Steps taken so far.
    pub steps: u32,
    pub usage: Usage,
    /// The agent's final answer, once there is one.
    pub reply: Option<String>,
    /// A page that shows the agent's screen live (noVNC), when there is one.
    pub live_url: Option<String>,
}

/// One step of the loop: the model's note, the action, and what came of it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Step {
    /// 1, 2, 3, ...
    pub n: u32,
    /// What the model said it learned and what's left.
    pub note: String,
    pub action: Action,
    /// The input was delivered. Whether it worked shows in the next observation.
    pub ok: bool,
    /// Why the action was refused, if it was.
    pub refused: Option<crate::Refusal>,
    /// `GET` this for the screenshot taken after the step.
    pub screenshot_url: Option<String>,
    /// RFC 3339, UTC.
    pub at: String,
}

/// What `GET /api/runs/{id}/events` (a WebSocket) sends, one JSON text frame each.
/// A new connection first gets every event so far, then live ones. The socket
/// closes after `finished`; on any other close, reconnect and replay.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RunEvent {
    Status {
        status: RunStatus,
    },
    Step {
        step: Step,
    },
    Usage {
        usage: Usage,
    },
    Finished {
        status: RunStatus,
        reply: Option<String>,
    },
    Error {
        message: String,
    },
}

/// A configured brain, as `GET /api/providers` lists it. Never carries a key.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderInfo {
    /// The name used in `StartRun.provider`, e.g. `openrouter`.
    pub name: String,
    /// The adapter: `anthropic`, `openai`, `openai_compat`, `gemini`, `codex`, `claude_code`, `antigravity`, `mock`.
    pub kind: String,
    pub model: String,
    pub vision: bool,
    pub native_computer: bool,
    pub is_default: bool,
}

/// `GET /api/health`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Health {
    pub version: String,
    /// [`crate::CONTRACT_VERSION`] of the server.
    pub contract: u32,
    /// True when the server is `harness serve --mock`.
    pub mock: bool,
}

/// The body of every 4xx/5xx answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApiError {
    pub error: String,
}
