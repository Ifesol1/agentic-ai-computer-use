//! Window mode: borrow one window on a member's PC. Windows, macOS and Linux are
//! equals: each file has the same shape and its own owner, and a feature counts as
//! done only when all three pass the same fixture tests (plan, section 10.2).

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
pub use linux::WindowMode;
#[cfg(target_os = "macos")]
pub use macos::WindowMode;
#[cfg(target_os = "windows")]
pub use windows::WindowMode;

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
compile_error!("hands supports Windows, macOS and Linux");

/// The OS this build drives, as observations and logs name it.
pub fn platform() -> &'static str {
    std::env::consts::OS
}

#[cfg(test)]
mod tests {
    use crate::Hands;

    #[tokio::test]
    async fn window_mode_says_it_is_not_built() {
        let hands = super::WindowMode::new("some-window");
        let error = hands.observe().await.unwrap_err().to_string();
        assert!(error.contains(super::platform()), "{error}");
    }
}
