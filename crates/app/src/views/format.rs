//! Every piece of text the UI shows: how runs, actions, statuses and the
//! connection read. Pure functions and constants, so they're tested without
//! opening a window.

use proto::{Action, ElementOp, Refusal, RunInfo, RunStatus, Usage};

pub const PROMPT_PLACEHOLDER: &str = "What should the agent do?";
pub const ENTER_TO_START: &str = "Enter to start";
pub const START_RUN: &str = "Start run";
pub const STOP: &str = "Stop";
pub const NO_RUN_SELECTED: &str = "Start a run, or pick one on the left.";

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
    format!(
        "{} · {} · {}",
        status(run.status),
        steps(run.steps),
        cost(run.usage.cost_usd)
    )
}

/// `1 step`, `4 steps`.
pub fn steps(n: u32) -> String {
    match n {
        1 => "1 step".to_owned(),
        n => format!("{n} steps"),
    }
}

/// Input and output tokens together, e.g. `5840 tokens`.
pub fn tokens(usage: &Usage) -> String {
    let total = u64::from(usage.input_tokens) + u64::from(usage.output_tokens);
    format!("{total} tokens")
}

pub fn cost(usd: f64) -> String {
    format!("${usd:.4}")
}

/// Which model ran it, e.g. `mock · scripted`.
pub fn provider_model(run: &RunInfo) -> String {
    format!("{} · {}", run.provider, run.model)
}

/// The connection line at the bottom of the sidebar.
pub fn connecting(url: &str) -> String {
    format!("Connecting to {url}")
}

pub fn connected(url: &str, mock: bool) -> String {
    match mock {
        true => format!("Mock backend · {url}"),
        false => format!("Backend · {url}"),
    }
}

pub fn unreachable(url: &str) -> String {
    format!("Can't reach {url}")
}

pub const START_BACKEND_HINT: &str = "Start it: cargo run -p agent -- serve --mock";

/// A warning when the backend speaks another contract version than this app.
pub fn contract_mismatch(backend: u32) -> Option<String> {
    (backend != proto::CONTRACT_VERSION).then(|| {
        format!(
            "Backend speaks contract v{backend}, this app v{}: pull and rebuild",
            proto::CONTRACT_VERSION
        )
    })
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

/// Next to a step the backend refused, e.g. `refused: stale observation`.
pub fn refused(why: Refusal) -> String {
    format!("refused: {}", refusal(why))
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
        assert_eq!(provider_model(&run), "mock · scripted");
    }

    #[test]
    fn counts_read_naturally() {
        assert_eq!(steps(0), "0 steps");
        assert_eq!(steps(1), "1 step");
        assert_eq!(steps(4), "4 steps");
        let usage = Usage {
            input_tokens: 5600,
            output_tokens: 240,
            cost_usd: 0.018,
        };
        assert_eq!(tokens(&usage), "5840 tokens");
        assert_eq!(cost(usage.cost_usd), "$0.0180");
    }

    #[test]
    fn connection_lines() {
        let url = "http://127.0.0.1:7878";
        assert_eq!(connecting(url), "Connecting to http://127.0.0.1:7878");
        assert_eq!(connected(url, true), "Mock backend · http://127.0.0.1:7878");
        assert_eq!(connected(url, false), "Backend · http://127.0.0.1:7878");
        assert_eq!(unreachable(url), "Can't reach http://127.0.0.1:7878");
        assert_eq!(contract_mismatch(proto::CONTRACT_VERSION), None);
        let other = proto::CONTRACT_VERSION + 1;
        assert!(
            contract_mismatch(other)
                .unwrap()
                .contains(&format!("v{other}"))
        );
    }

    #[test]
    fn refusals_read_as_text() {
        assert_eq!(refused(Refusal::StaleToken), "refused: stale observation");
    }
}
