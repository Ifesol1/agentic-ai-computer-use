//! Window mode on macOS.
//!
//! - Capture one window: ScreenCaptureKit with a window filter (needs Screen Recording permission).
//! - Read it as text: AX, the Accessibility API (needs Accessibility permission).
//! - Act without the mouse: AX actions (AXPress, set AXValue).
//! - Crates to try: the `objc2` family, `screencapturekit`.
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
            "window mode on macos isn't built yet (window {})",
            self.window
        )
    }

    async fn act(&self, _token: &str, _action: &Action) -> anyhow::Result<ActionResult> {
        bail!(
            "window mode on macos isn't built yet (window {})",
            self.window
        )
    }
}
