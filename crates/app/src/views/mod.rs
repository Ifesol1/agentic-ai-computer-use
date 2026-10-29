//! One file per view, so several people can work on the UI at once.
//!
//! - `workspace`: the window's layout; polls the backend and wires the views together
//! - `new_run`: the prompt box and Start button
//! - `run_list`: the runs, newest first
//! - `run_detail`: one run, live: steps, usage, reply, Stop
//! - `format`: how runs, actions and statuses read as text (unit-tested)

mod format;
mod new_run;
mod run_detail;
mod run_list;
mod workspace;

pub use workspace::Workspace;

use gpui_kit::component::ActiveTheme;
use gpui_kit::*;
use proto::RunStatus;

/// The small round status marker used in the list and the detail header.
fn dot(color: Hsla) -> Div {
    div().size(px(8.)).flex_none().rounded_full().bg(color)
}

fn status_color(status: RunStatus, cx: &App) -> Hsla {
    let theme = cx.theme();
    match status {
        RunStatus::Queued => theme.muted_foreground,
        RunStatus::Running => theme.info,
        RunStatus::Succeeded => theme.success,
        RunStatus::Failed => theme.danger,
        RunStatus::Stopped => theme.warning,
    }
}
