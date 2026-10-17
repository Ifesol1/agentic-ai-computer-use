//! A backend that plays scripted runs: no keys, no models, no desktop.
//!
//! The frontend builds against this. Every API feature lands here too, so the
//! frontend never waits on the real backend. Mock tricks for testing the UI:
//! a prompt containing "fail" ends in a failed run, and `max_steps` below 4 ends
//! in a stopped one.

use std::{
    sync::{Arc, Mutex},
    time::{Duration, SystemTime},
};

use async_trait::async_trait;
use proto::{
    Action, ElementOp, ProviderInfo, Refusal, RunEvent, RunInfo, RunStatus, StartRun, Step, Usage,
};
use tokio::{sync::broadcast, task::AbortHandle};

use crate::{Backend, BadRequest};

pub struct MockBackend {
    state: Arc<Mutex<State>>,
    step_delay: Duration,
}

#[derive(Default)]
struct State {
    /// Oldest first.
    runs: Vec<Run>,
}

struct Run {
    info: RunInfo,
    events: Vec<RunEvent>,
    fails: bool,
    /// `None` once the run is final, which closes every subscriber.
    live: Option<broadcast::Sender<RunEvent>>,
    task: Option<AbortHandle>,
}

impl State {
    fn run(&self, id: &str) -> Option<&Run> {
        self.runs.iter().find(|run| run.info.id == id)
    }

    fn run_mut(&mut self, id: &str) -> Option<&mut Run> {
        self.runs.iter_mut().find(|run| run.info.id == id)
    }
}

impl Run {
    fn emit(&mut self, mut event: RunEvent) {
        match &mut event {
            RunEvent::Status { status } => self.info.status = *status,
            RunEvent::Step { step } => {
                step.at = now();
                self.info.steps = step.n;
            }
            RunEvent::Usage { usage } => self.info.usage = *usage,
            RunEvent::Finished { status, reply } => {
                self.info.status = *status;
                self.info.reply = reply.clone();
            }
            RunEvent::Error { .. } => {}
        }
        if let Some(live) = &self.live {
            let _ = live.send(event.clone());
        }
        self.events.push(event);
        if self.info.status.is_final() {
            self.live = None;
            self.task = None;
        }
    }
}

impl MockBackend {
    /// `step_delay` is the pause before each step; tests use zero.
    pub fn new(step_delay: Duration) -> Self {
        let backend = Self {
            state: Arc::default(),
            step_delay,
        };
        // One finished run, so a fresh UI has something to show.
        let demo = StartRun {
            prompt: "Open Settings and turn on dark mode".into(),
            provider: None,
            model: None,
            target: None,
            max_steps: None,
            max_cost_usd: None,
        };
        let mut state = backend.state.lock().unwrap();
        let id = backend
            .insert(&mut state, &demo)
            .expect("the demo run is valid");
        let run = state.run_mut(&id).unwrap();
        for event in script(&run.info, run.fails, None) {
            run.emit(event);
        }
        drop(state);
        backend
    }

    fn insert(&self, state: &mut State, req: &StartRun) -> Result<String, BadRequest> {
        if req.prompt.trim().is_empty() {
            return Err(BadRequest("prompt is empty".into()));
        }
        if req.max_steps == Some(0) {
            return Err(BadRequest("max_steps must be at least 1".into()));
        }
        let providers = providers();
        let provider = match &req.provider {
            Some(name) => providers
                .iter()
                .find(|p| &p.name == name)
                .ok_or_else(|| BadRequest(format!("no provider {name}; see GET /api/providers")))?,
            None => providers.iter().find(|p| p.is_default).unwrap(),
        };
        let id = format!("run-{}", state.runs.len() + 1);
        let (live, _) = broadcast::channel(64);
        state.runs.push(Run {
            info: RunInfo {
                id: id.clone(),
                prompt: req.prompt.clone(),
                provider: provider.name.clone(),
                model: req.model.clone().unwrap_or_else(|| provider.model.clone()),
                target: req.target.clone().unwrap_or_default(),
                status: RunStatus::Queued,
                created_at: now(),
                steps: 0,
                usage: Usage::default(),
                reply: None,
                live_url: None,
            },
            events: Vec::new(),
            fails: req.prompt.to_lowercase().contains("fail"),
            live: Some(live),
            task: None,
        });
        Ok(id)
    }
}

#[async_trait]
impl Backend for MockBackend {
    fn is_mock(&self) -> bool {
        true
    }

    async fn providers(&self) -> Vec<ProviderInfo> {
        providers()
    }

    async fn list_runs(&self) -> Vec<RunInfo> {
        let state = self.state.lock().unwrap();
        state
            .runs
            .iter()
            .rev()
            .map(|run| run.info.clone())
            .collect()
    }

    async fn get_run(&self, id: &str) -> Option<RunInfo> {
        self.state
            .lock()
            .unwrap()
            .run(id)
            .map(|run| run.info.clone())
    }

    async fn start_run(&self, req: StartRun) -> Result<RunInfo, BadRequest> {
        let mut state = self.state.lock().unwrap();
        let id = self.insert(&mut state, &req)?;
        let run = state.run_mut(&id).unwrap();
        let events = script(&run.info, run.fails, req.max_steps);
        let task = tokio::spawn(play(self.state.clone(), id, events, self.step_delay));
        run.task = Some(task.abort_handle());
        Ok(run.info.clone())
    }

    async fn stop_run(&self, id: &str) -> Option<RunInfo> {
        let mut state = self.state.lock().unwrap();
        let run = state.run_mut(id)?;
        if !run.info.status.is_final() {
            if let Some(task) = run.task.take() {
                task.abort();
            }
            run.emit(RunEvent::Finished {
                status: RunStatus::Stopped,
                reply: None,
            });
        }
        Some(run.info.clone())
    }

    async fn subscribe(&self, id: &str) -> Option<(Vec<RunEvent>, broadcast::Receiver<RunEvent>)> {
        let state = self.state.lock().unwrap();
        let run = state.run(id)?;
        let live = match &run.live {
            Some(live) => live.subscribe(),
            None => {
                let (closed, live) = broadcast::channel(1);
                drop(closed);
                live
            }
        };
        Some((run.events.clone(), live))
    }

    async fn screenshot(&self, id: &str, n: u32) -> Option<(&'static str, Vec<u8>)> {
        let state = self.state.lock().unwrap();
        let run = state.run(id)?;
        if n == 0 || n > run.info.steps {
            return None;
        }
        let acted_on = |target: &str| {
            run.events.iter().any(|event| match event {
                RunEvent::Step { step } => {
                    step.n <= n
                        && matches!(&step.action, Action::Act { element, .. } if element == target)
                }
                _ => false,
            })
        };
        let opened = acted_on("e3");
        let dark = opened && acted_on("e12") && !run.fails;
        Some(("image/svg+xml", settings_svg(n, opened, dark).into_bytes()))
    }
}

/// Sends the scripted events in order, pausing before each step, until the run
/// finishes or is stopped.
async fn play(state: Arc<Mutex<State>>, id: String, events: Vec<RunEvent>, delay: Duration) {
    for event in events {
        if matches!(event, RunEvent::Step { .. } | RunEvent::Finished { .. }) {
            tokio::time::sleep(delay).await;
        }
        let mut guard = state.lock().unwrap();
        let Some(run) = guard.run_mut(&id) else {
            return;
        };
        if run.info.status.is_final() {
            return;
        }
        run.emit(event);
    }
}

/// "Turn on dark mode" in a fake Settings app, as events.
fn script(run: &RunInfo, fails: bool, max_steps: Option<u32>) -> Vec<RunEvent> {
    let act = |element: &str| Action::Act {
        element: element.into(),
        op: ElementOp::Click,
        text: None,
    };
    let mut steps = vec![
        ("Looking at the Settings window.", Action::Observe, None),
        (
            "Appearance is in the sidebar (e3). Opening it.",
            act("e3"),
            None,
        ),
        (
            "The Dark mode switch (e12) is off. Turning it on.",
            act("e12"),
            None,
        ),
        (
            "Checking that the switch now shows on.",
            Action::Observe,
            None,
        ),
    ];
    let (status, reply) = if fails {
        steps.push((
            "The switch still shows off. Trying once more.",
            act("e12"),
            Some(Refusal::NoVisibleProgress),
        ));
        (
            RunStatus::Failed,
            "Couldn't turn on dark mode: the switch didn't change.",
        )
    } else {
        (
            RunStatus::Succeeded,
            "Dark mode is on: the Appearance page shows the switch on.",
        )
    };

    let mut events = vec![RunEvent::Status {
        status: RunStatus::Running,
    }];
    let mut usage = Usage::default();
    for (i, (note, action, refused)) in steps.into_iter().enumerate() {
        let n = i as u32 + 1;
        if max_steps.is_some_and(|max| n > max) {
            events.push(RunEvent::Finished {
                status: RunStatus::Stopped,
                reply: Some(format!(
                    "Stopped after {} steps: the step budget ran out.",
                    n - 1
                )),
            });
            return events;
        }
        usage.input_tokens += 1_400;
        usage.output_tokens += 60;
        usage.cost_usd += 0.0045;
        events.push(RunEvent::Step {
            step: Step {
                n,
                note: note.into(),
                action,
                ok: refused.is_none(),
                refused,
                screenshot_url: Some(format!("/api/runs/{}/steps/{n}/screenshot", run.id)),
                at: now(),
            },
        });
        events.push(RunEvent::Usage { usage });
    }
    events.push(RunEvent::Finished {
        status,
        reply: Some(reply.into()),
    });
    events
}

fn providers() -> Vec<ProviderInfo> {
    let provider =
        |name: &str, kind: &str, model: &str, native_computer, is_default| ProviderInfo {
            name: name.into(),
            kind: kind.into(),
            model: model.into(),
            vision: true,
            native_computer,
            is_default,
        };
    vec![
        provider("mock", "mock", "scripted", false, true),
        provider("claude", "anthropic", "claude-sonnet-5-5", true, false),
        provider(
            "local",
            "openai_compat",
            "a local vision model",
            false,
            false,
        ),
    ]
}

fn now() -> String {
    humantime::format_rfc3339_seconds(SystemTime::now()).to_string()
}

/// A fake Settings window as it looks after step `n`.
fn settings_svg(n: u32, opened: bool, dark: bool) -> String {
    let (bg, fg, panel) = if dark {
        ("#1e1f22", "#e8e8ea", "#2a2b2f")
    } else {
        ("#ffffff", "#1d1d1f", "#f2f2f5")
    };
    let page = if opened {
        let (knob, track) = if dark {
            (1102, "#3b82f6")
        } else {
            (1074, "#b0b0b8")
        };
        format!(
            r##"<text x="320" y="130" font-size="34" font-weight="600" fill="{fg}">Appearance</text>
<text x="320" y="210" font-size="22" fill="{fg}">Dark mode</text>
<rect x="1060" y="186" width="70" height="36" rx="18" fill="{track}"/>
<circle cx="{knob}" cy="204" r="14" fill="#ffffff"/>
<text x="320" y="290" font-size="22" fill="{fg}">Text size</text>
<text x="1130" y="290" font-size="22" text-anchor="end" fill="{fg}">100%</text>"##
        )
    } else {
        format!(
            r##"<text x="320" y="130" font-size="34" font-weight="600" fill="{fg}">Wi-Fi</text>
<text x="320" y="210" font-size="22" fill="{fg}">Connected to Club-Lab</text>"##
        )
    };
    let selected = if opened { 154 } else { 74 };
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="800" viewBox="0 0 1280 800" font-family="sans-serif">
<rect width="1280" height="800" fill="{bg}"/>
<rect width="260" height="800" fill="{panel}"/>
<rect x="12" y="{selected}" width="236" height="44" rx="8" fill="#3b82f6" fill-opacity="0.25"/>
<text x="32" y="104" font-size="20" fill="{fg}">Wi-Fi</text>
<text x="32" y="144" font-size="20" fill="{fg}">Bluetooth</text>
<text x="32" y="184" font-size="20" fill="{fg}">Appearance</text>
<text x="32" y="224" font-size="20" fill="{fg}">Sound</text>
{page}
<text x="1260" y="780" font-size="16" text-anchor="end" fill="{fg}" fill-opacity="0.5">mock · step {n}</text>
</svg>"##
    )
}
