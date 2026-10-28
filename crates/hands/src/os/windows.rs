//! Window mode on Windows.
//!
//! - Capture one window: Windows.Graphics.Capture.
//! - Read it as text: UI Automation.
//! - Act without the mouse: UIA patterns (Invoke, Value, Toggle, Scroll). Admin windows need an admin agent: keep it non-admin.
//! - Crates to try: `windows`, `uiautomation`.
//!
//! Not built yet. Keep the public shape (`WindowMode::new` + `Hands`) the same as
//! the other two OS files.

use anyhow::bail;
use async_trait::async_trait;
use proto::{Action, ActionResult, Observation};

use crate::Hands;

pub struct WindowMode {
    window: String,
}

impl WindowMode {
    /// `window` is picked by the person (or the hub), never by the model.
    pub fn new(window: impl Into<String>) -> Self {
        Self {
            window: window.into(),
        }
    }
}

#[async_trait]
impl Hands for WindowMode {
    async fn observe(&self) -> anyhow::Result<Observation> {
        bail!(
            "window mode on windows isn't built yet (window {})",
            self.window
        )
    }

    async fn act(&self, _token: &str, _action: &Action) -> anyhow::Result<ActionResult> {
        bail!(
            "window mode on windows isn't built yet (window {})",
            self.window
        )
    }
}
