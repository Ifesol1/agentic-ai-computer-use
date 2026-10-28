//! Window mode on Linux.
//!
//! - Capture one window: X11: XComposite; Wayland: the ScreenCast portal (asks the person).
//! - Read it as text: AT-SPI.
//! - Act without the mouse: AT-SPI Action and EditableText.
//! - Crates to try: `atspi`, `x11rb`.
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
            "window mode on linux isn't built yet (window {})",
            self.window
        )
    }

    async fn act(&self, _token: &str, _action: &Action) -> anyhow::Result<ActionResult> {
        bail!(
            "window mode on linux isn't built yet (window {})",
            self.window
        )
    }
}
