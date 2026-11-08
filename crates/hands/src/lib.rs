//! Screens and input. Each backend (the Docker desktop, window mode on Windows,
//! macOS and Linux, the fake desktop for tests) implements [`Hands`]; the agent
//! never knows which one it has.
//!
//! - `os/`: window mode, one file per OS, each with its own owner. Only the file
//!   for the OS being built is compiled, so CI on all three runners checks all three.
//! - `x11` (Linux): a whole X11 desktop, such as the Docker Xvfb private desktop:
//!   screenshots and mouse and keyboard over the whole screen, tested on Xvfb.
//! - `fake`: a scripted Settings window, for tests and `harness run --mock`.
//! - `scale`: image <-> screen coordinates, tested.

pub mod fake;
mod os;
pub mod scale;
#[cfg(target_os = "linux")]
pub mod x11;

pub use os::{WindowMode, platform};

use async_trait::async_trait;
use proto::{Action, ActionResult, Observation};

#[async_trait]
pub trait Hands: Send + Sync {
    /// Look at the window (or private desktop) this session was given.
    async fn observe(&self) -> anyhow::Result<Observation>;
    /// Do one action. `token` is from the newest observation; a stale one is
    /// refused with a fresh observation, never acted on. `Err` only when the
    /// backend itself broke (no observation could be taken).
    async fn act(&self, token: &str, action: &Action) -> anyhow::Result<ActionResult>;
}

/// The private desktop on X11 `display` (e.g. `:99`), where this OS has one.
pub fn desktop(display: &str) -> anyhow::Result<Box<dyn Hands>> {
    #[cfg(target_os = "linux")]
    {
        Ok(Box::new(x11::Desktop::connect(display)?))
    }
    #[cfg(not(target_os = "linux"))]
    {
        anyhow::bail!(
            "the X11 desktop isn't available on {}: it runs on Linux (display {display})",
            platform()
        )
    }
}

/// Whether an action changes the window, so its result needs a look before the
/// run can count it as done.
pub fn needs_verify(action: &Action) -> bool {
    !matches!(action, Action::Observe | Action::Read | Action::Wait { .. })
}
