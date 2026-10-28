//! How runs, actions and statuses read as text. Pure functions, so they're tested
//! without opening a window.

use proto::{Action, ElementOp, Refusal, RunInfo, RunStatus};

pub fn status(status: RunStatus) -> &'static str {
    match status {
        RunStatus::Queued => "queued",
        RunStatus::Running => "running",
        RunStatus::Succeeded => "succeeded",
        RunStatus::Failed => "failed",
        RunStatus::Stopped => "stopped",
    }
}

/// One line under a run in the list, e.g. `succeeded · 4 steps · $0.0180`.
pub fn run_summary(run: &RunInfo) -> String {
    let steps = match run.steps {
        1 => "1 step".to_owned(),
        n => format!("{n} steps"),
    };
    format!("{} · {steps} · ${:.4}", status(run.status), run.usage.cost_usd)
}

pub fn action(action: &Action) -> String {
    match action {
        Action::Observe => "observe".into(),
        Action::Act { element, op, text } => {
            let op = match op {
                ElementOp::Click => "click",
                ElementOp::DoubleClick => "double-click",
                ElementOp::RightClick => "right-click",
                ElementOp::Type => "type into",
                ElementOp::Scroll => "scroll",
            };
            match text {
                Some(text) => format!("{op} {element}: \"{text}\""),
                None => format!("{op} {element}"),
            }
        }
        Action::Click { x, y, count, .. } if *count > 1 => format!("double-click ({x}, {y})"),
        Action::Click { x, y, .. } => format!("click ({x}, {y})"),
        Action::Move { x, y } => format!("move to ({x}, {y})"),
        Action::Scroll { x, y, dx, dy } => format!("scroll ({dx}, {dy}) at ({x}, {y})"),
        Action::Drag { from, to, .. } => {
            format!("drag ({}, {}) to ({}, {})", from.x, from.y, to.x, to.y)
        }
        Action::Key { keys } => format!("key {keys}"),
        Action::TypeText { text } => format!("type \"{text}\""),
        Action::Read => "read the window".into(),
        Action::Wait { ms } => format!("wait {ms} ms"),
    }
}

pub fn refusal(refusal: Refusal) -> &'static str {
    match refusal {
        Refusal::StaleToken => "stale observation",
        Refusal::WindowChanged => "the window changed",
        Refusal::NoVisibleProgress => "no visible progress",
        Refusal::AppInUse => "the person is using this app",
        Refusal::Busy => "another agent is busy here",
        Refusal::OutOfBounds => "outside the window",
        Refusal::BudgetSpent => "budget spent",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proto::{Target, Usage};

    #[test]
    fn actions_read_as_text() {
        let act = Action::Act {
            element: "e12".into(),
            op: ElementOp::Click,
            text: None,
        };
        assert_eq!(action(&act), "click e12");
        assert_eq!(
            action(&Action::TypeText { text: "hi".into() }),
            "type \"hi\""
        );
    }

    #[test]
    fn run_summary_counts_steps() {
        let run = RunInfo {
            id: "run-1".into(),
            prompt: "x".into(),
            provider: "mock".into(),
            model: "scripted".into(),
            target: Target::PrivateDesktop,
            status: RunStatus::Succeeded,
            created_at: String::new(),
            steps: 1,
            usage: Usage {
                cost_usd: 0.0045,
                ..Usage::default()
            },
            reply: None,
            live_url: None,
        };
        assert_eq!(run_summary(&run), "succeeded · 1 step · $0.0045");
    }
}
