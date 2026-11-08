//! The backend that runs the real agent loop behind the API: what `harness serve`
//! serves without `--mock`.
//!
//! It keeps runs in memory the way `api::MockBackend` does (info, the events so
//! far, a broadcast channel for live subscribers), but each run is a tokio task
//! running [`crate::run`] with a real provider and real hands. Which provider and
//! which hands come from factories the caller gives it, so config, window mode
//! and the Docker desktop plug in without this file changing.
//!
//! Stop is cooperative: it cancels the run's token and the loop ends the run
//! `Stopped` after the action in flight. Screenshots the loop hands over are
//! kept per step and served from the screenshot route.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime},
};

use api::{Backend, BadRequest};
use async_trait::async_trait;
use base64::Engine;
use hands::Hands;
use llm::Provider;
use proto::{ProviderInfo, RunEvent, RunInfo, RunStatus, StartRun, Target, Usage};
use tokio::sync::{Notify, broadcast};

use crate::{CancellationToken, Limits, Sink, Task};

/// Builds a fresh provider for one run.
pub type ProviderFactory = Arc<dyn Fn() -> anyhow::Result<Box<dyn Provider>> + Send + Sync>;
/// Builds the hands for one run's target.
pub type HandsFactory = Arc<dyn Fn(&Target) -> anyhow::Result<Box<dyn Hands>> + Send + Sync>;

/// One configured provider, as the API lists it and as a run builds it.
pub struct ProviderSpec {
    pub info: ProviderInfo,
    pub build: ProviderFactory,
}

pub struct AgentBackend {
    state: Arc<Mutex<State>>,
    providers: Vec<ProviderSpec>,
    hands: HandsFactory,
}

#[derive(Default)]
struct State {
    /// Oldest first.
    runs: Vec<Run>,
}

struct Run {
    info: RunInfo,
    events: Vec<RunEvent>,
    /// `None` once the run is final, which closes every subscriber.
    live: Option<broadcast::Sender<RunEvent>>,
    stop: CancellationToken,
    /// Notified once, when the run turns final.
    finished: Arc<Notify>,
    screenshots: HashMap<u32, (&'static str, Vec<u8>)>,
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
            RunEvent::Step { step } => self.info.steps = step.n,
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
            self.finished.notify_one();
        }
    }
}

impl AgentBackend {
    /// `providers` must have exactly one default.
    pub fn new(providers: Vec<ProviderSpec>, hands: HandsFactory) -> Self {
        assert_eq!(
            providers.iter().filter(|p| p.info.is_default).count(),
            1,
            "exactly one default provider"
        );
        Self {
            state: Arc::default(),
            providers,
            hands,
        }
    }

    /// The real loop on the scripted model and the fake Settings window: what
    /// `harness serve` runs until there is config for real providers and hands.
    pub fn demo() -> Self {
        Self::demo_on(Arc::new(|_| Ok(Box::new(hands::fake::FakeSettings::new()))))
    }

    /// The scripted model on whatever `hands` builds: the real desktop, say.
    pub fn demo_on(hands: HandsFactory) -> Self {
        Self::new(vec![crate::config::mock_spec(true)], hands)
    }

    fn provider(&self, name: Option<&str>) -> Result<&ProviderSpec, BadRequest> {
        match name {
            Some(name) => self
                .providers
                .iter()
                .find(|p| p.info.name == name)
                .ok_or_else(|| BadRequest(format!("no provider {name}; see GET /api/providers"))),
            None => Ok(self.providers.iter().find(|p| p.info.is_default).unwrap()),
        }
    }
}

#[async_trait]
impl Backend for AgentBackend {
    async fn providers(&self) -> Vec<ProviderInfo> {
        self.providers.iter().map(|p| p.info.clone()).collect()
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
        if req.prompt.trim().is_empty() {
            return Err(BadRequest("prompt is empty".into()));
        }
        if req.max_steps == Some(0) {
            return Err(BadRequest("max_steps must be at least 1".into()));
        }
        let spec = self.provider(req.provider.as_deref())?;
        let model = req.model.clone().unwrap_or_else(|| spec.info.model.clone());
        let target = req.target.clone().unwrap_or_default();
        let defaults = Limits::default();
        let task = Task {
            prompt: req.prompt.clone(),
            model: model.clone(),
            limits: Limits {
                max_turns: req.max_steps.unwrap_or(defaults.max_turns),
                max_cost_usd: req.max_cost_usd.unwrap_or(defaults.max_cost_usd),
            },
        };

        let stop = CancellationToken::new();
        let (live, _) = broadcast::channel(64);
        let info = {
            let mut state = self.state.lock().unwrap();
            let info = RunInfo {
                id: format!("run-{}", state.runs.len() + 1),
                prompt: req.prompt,
                provider: spec.info.name.clone(),
                model,
                target: target.clone(),
                status: RunStatus::Queued,
                created_at: now(),
                steps: 0,
                usage: Usage::default(),
                reply: None,
                live_url: None,
            };
            state.runs.push(Run {
                info: info.clone(),
                events: Vec::new(),
                live: Some(live),
                stop: stop.clone(),
                finished: Arc::default(),
                screenshots: HashMap::new(),
            });
            info
        };

        let mut feed = Feed {
            state: self.state.clone(),
            id: info.id.clone(),
        };
        let build_provider = spec.build.clone();
        let build_hands = self.hands.clone();
        tokio::spawn(async move {
            let built = build_provider().and_then(|provider| Ok((provider, build_hands(&target)?)));
            match built {
                Ok((provider, hands)) => {
                    crate::run(provider.as_ref(), hands.as_ref(), &task, &stop, &mut feed).await;
                }
                Err(error) => {
                    feed.event(RunEvent::Error {
                        message: format!("{error:#}"),
                    });
                    feed.event(RunEvent::Finished {
                        status: RunStatus::Failed,
                        reply: None,
                    });
                }
            }
        });
        Ok(info)
    }

    async fn stop_run(&self, id: &str) -> Option<RunInfo> {
        let (stop, finished, is_final) = {
            let state = self.state.lock().unwrap();
            let run = state.run(id)?;
            (
                run.stop.clone(),
                run.finished.clone(),
                run.info.status.is_final(),
            )
        };
        if !is_final {
            stop.cancel();
            // The loop ends the run after the action in flight; give it a moment
            // so the answer already says `stopped`. A longer action is reported
            // as still running and finishes on its own.
            let _ = tokio::time::timeout(Duration::from_secs(5), finished.notified()).await;
        }
        self.get_run(id).await
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
        state.run(id)?.screenshots.get(&n).cloned()
    }
}

/// The loop's sink for one run: events into the run, screenshots kept by step.
struct Feed {
    state: Arc<Mutex<State>>,
    id: String,
}

impl Sink for Feed {
    fn event(&mut self, event: RunEvent) {
        let mut state = self.state.lock().unwrap();
        if let Some(run) = state.run_mut(&self.id) {
            run.emit(event);
        }
    }

    fn screenshot(&mut self, n: u32, image: &str) -> Option<String> {
        // A URL the hands already serve is passed through as it is.
        let Some((content_type, bytes)) = decode_data_url(image) else {
            return Some(image.to_owned());
        };
        let mut state = self.state.lock().unwrap();
        let run = state.run_mut(&self.id)?;
        run.screenshots.insert(n, (content_type, bytes));
        Some(format!("/api/runs/{}/steps/{n}/screenshot", self.id))
    }
}

/// `data:<type>;base64,<data>` as (content type, bytes). `None` for anything else.
fn decode_data_url(url: &str) -> Option<(&'static str, Vec<u8>)> {
    let rest = url.strip_prefix("data:")?;
    let (meta, data) = rest.split_once(',')?;
    let media_type = meta.strip_suffix(";base64")?;
    let content_type = match media_type {
        "image/png" => "image/png",
        "image/jpeg" => "image/jpeg",
        "image/webp" => "image/webp",
        "image/gif" => "image/gif",
        "image/svg+xml" => "image/svg+xml",
        _ => "application/octet-stream",
    };
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data.trim())
        .ok()?;
    Some((content_type, bytes))
}

fn now() -> String {
    humantime::format_rfc3339_seconds(SystemTime::now()).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_urls_decode_and_others_pass_through() {
        let (content_type, bytes) = decode_data_url("data:image/png;base64,aGVsbG8=").unwrap();
        assert_eq!(content_type, "image/png");
        assert_eq!(bytes, b"hello");
        assert_eq!(decode_data_url("https://example.com/shot.png"), None);
        assert_eq!(decode_data_url("data:image/png;base64,***"), None);
        assert_eq!(decode_data_url("data:image/png,plain"), None);
    }
}
