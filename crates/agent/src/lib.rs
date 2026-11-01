//! The agent loop: observe -> model -> one action -> fresh observation -> repeat.
//!
//! The loop speaks only `proto` types and the `llm::Provider` / `hands::Hands`
//! traits, so it never knows which model or which OS it drives. The rules it
//! keeps (plan, section 10):
//!
//! - Every action returns a fresh observation, and the model sees it before the
//!   run can end: `done` in the same turn as an action is refused.
//! - The loop, not the model, holds the observation token. A refused action is
//!   reported back with fresh evidence; nothing is retried automatically.
//! - Budgets (turns, cost) are the caller's; the model can't raise them.
//! - A person can stop a run at any time (a [`CancellationToken`]). A stop waits
//!   for the action in flight, never for the model, and ends the run `Stopped`.
//! - Progress goes out as `proto::RunEvent`s, the same events the app shows,
//!   through a [`Sink`], which also gets each step's screenshot to keep.

pub mod demo;
mod tool;
mod tools;

pub use tokio_util::sync::CancellationToken;
pub use tool::{Call, Tool, all as all_tools, find as find_tool};

use std::time::SystemTime;

use hands::Hands;
use llm::{Provider, TurnRequest};
use proto::{
    ActionResult, ContentBlock, Message, Observation, RunEvent, RunStatus, Step, ToolSpec, Usage,
};

/// Where a run's progress goes: the events the app shows, and the screenshots
/// behind them. A plain `FnMut(RunEvent)` is a sink that keeps no screenshots.
pub trait Sink: Send {
    fn event(&mut self, event: RunEvent);

    /// The observation image taken after step `n` (a URL or data URL, as in
    /// [`Observation::image`]). Returns where it can now be fetched, which the
    /// loop puts in the step's `screenshot_url`; `None` keeps nothing.
    fn screenshot(&mut self, _n: u32, _image: &str) -> Option<String> {
        None
    }
}

impl<F: FnMut(RunEvent) + Send> Sink for F {
    fn event(&mut self, event: RunEvent) {
        self(event)
    }
}

/// The reply of a run a person stopped.
pub const STOPPED_BY_REQUEST: &str = "Stopped by request.";

const SYSTEM: &str = "You operate one window on a computer through tools. Each observation \
lists the window's controls (an outline); work them by reference with the `computer` tool. \
Do one action per turn. Before you call `done`, look at the newest observation and only \
claim what it shows.";

/// Budgets the model can't raise.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limits {
    /// Model turns, including ones whose call was refused.
    pub max_turns: u32,
    pub max_cost_usd: f64,
}

impl Default for Limits {
    /// A computer task's default budget (plan, section 10.6).
    fn default() -> Self {
        Self {
            max_turns: 64,
            max_cost_usd: 1.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Task {
    pub prompt: String,
    pub model: String,
    pub limits: Limits,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub status: RunStatus,
    pub reply: Option<String>,
    pub steps: u32,
    pub usage: Usage,
}

/// Runs one task to the end, or until `stop` is cancelled. Never panics on model
/// or backend errors: they end the run as `Failed`, reported through `sink` like
/// everything else.
pub async fn run(
    provider: &dyn Provider,
    hands: &dyn Hands,
    task: &Task,
    stop: &CancellationToken,
    sink: &mut dyn Sink,
) -> Outcome {
    let mut run = Run {
        sink,
        steps: 0,
        usage: Usage::default(),
    };
    run.emit(RunEvent::Status {
        status: RunStatus::Running,
    });
    match run.drive(provider, hands, task, stop).await {
        Ok((status, reply)) => run.finish(status, reply),
        Err(error) => {
            run.emit(RunEvent::Error {
                message: format!("{error:#}"),
            });
            run.finish(RunStatus::Failed, None)
        }
    }
}

struct Run<'a> {
    sink: &'a mut dyn Sink,
    steps: u32,
    usage: Usage,
}

impl Run<'_> {
    fn emit(&mut self, event: RunEvent) {
        self.sink.event(event);
    }

    fn finish(&mut self, status: RunStatus, reply: Option<String>) -> Outcome {
        self.emit(RunEvent::Finished {
            status,
            reply: reply.clone(),
        });
        Outcome {
            status,
            reply,
            steps: self.steps,
            usage: self.usage,
        }
    }

    async fn drive(
        &mut self,
        provider: &dyn Provider,
        hands: &dyn Hands,
        task: &Task,
        stop: &CancellationToken,
    ) -> anyhow::Result<(RunStatus, Option<String>)> {
        let stopped = || Ok((RunStatus::Stopped, Some(STOPPED_BY_REQUEST.to_owned())));
        if stop.is_cancelled() {
            return stopped();
        }
        let tools: Vec<ToolSpec> = all_tools().iter().map(|tool| tool.spec()).collect();
        let vision = provider.caps(&task.model).vision;
        let mut observation = hands.observe().await?;
        let mut first = vec![ContentBlock::text(format!("Task: {}", task.prompt))];
        first.extend(describe(&observation, vision));
        let mut messages = vec![Message::user(first)];

        for turn in 1.. {
            // Checked between turns, so a stop never cuts an action short.
            if stop.is_cancelled() {
                return stopped();
            }
            if turn > task.limits.max_turns {
                let reply = format!(
                    "Stopped: the budget of {} turns ran out.",
                    task.limits.max_turns
                );
                return Ok((RunStatus::Stopped, Some(reply)));
            }
            if self.usage.cost_usd >= task.limits.max_cost_usd {
                let reply = format!(
                    "Stopped: the ${:.2} budget ran out.",
                    task.limits.max_cost_usd
                );
                return Ok((RunStatus::Stopped, Some(reply)));
            }

            let request = TurnRequest {
                model: task.model.clone(),
                system: SYSTEM.into(),
                messages: messages.clone(),
                tools: tools.clone(),
            };
            // A stop doesn't wait for a slow model: the request is dropped.
            let response = tokio::select! {
                biased;
                () = stop.cancelled() => return stopped(),
                response = provider.turn(request) => response?,
            };
            self.usage.input_tokens += response.usage.input_tokens;
            self.usage.output_tokens += response.usage.output_tokens;
            self.usage.cost_usd += response.usage.cost_usd;
            self.emit(RunEvent::Usage { usage: self.usage });

            let note = texts(&response.content);
            let calls: Vec<_> = response
                .content
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::ToolCall { id, name, args } => Some((id, name, args)),
                    _ => None,
                })
                .collect();
            if calls.is_empty() {
                // A plain answer ends the run. Every earlier action's result was
                // already shown to the model, so it's answering from evidence.
                return Ok((RunStatus::Succeeded, Some(note)));
            }

            let mut results = Vec::new();
            let mut acted = false;
            for (id, name, args) in calls {
                let call = find_tool(name)
                    .ok_or_else(|| anyhow::anyhow!("no tool named `{name}`"))
                    .and_then(|tool| tool.parse(args));
                let result = match call {
                    Err(error) => refused(id, format!("{error:#}")),
                    Ok(_) if acted => refused(
                        id,
                        "Not run: one action per turn, and `done` only after you've seen the \
                         newest observation."
                            .into(),
                    ),
                    Ok(Call::Done { reply }) => return Ok((RunStatus::Succeeded, Some(reply))),
                    Ok(Call::Act(action)) => {
                        acted = true;
                        let outcome = hands.act(&observation.token, &action).await?;
                        self.steps += 1;
                        // The sink keeps the image first, so the URL in the step
                        // event already works when the app sees it.
                        let screenshot_url = outcome
                            .observation
                            .image
                            .as_deref()
                            .and_then(|image| self.sink.screenshot(self.steps, image));
                        self.emit(RunEvent::Step {
                            step: Step {
                                n: self.steps,
                                note: note.clone(),
                                action,
                                ok: outcome.ok,
                                refused: outcome.refused,
                                screenshot_url,
                                at: now(),
                            },
                        });
                        let content = report(&outcome, vision);
                        observation = outcome.observation;
                        ContentBlock::ToolResult {
                            id: id.clone(),
                            content,
                            is_error: false,
                        }
                    }
                };
                results.push(result);
            }
            messages.push(Message::assistant(response.content));
            messages.push(Message::user(results));
        }
        unreachable!("the turn budget ends the loop")
    }
}

fn refused(id: &str, why: String) -> ContentBlock {
    ContentBlock::ToolResult {
        id: id.to_owned(),
        content: vec![ContentBlock::text(why)],
        is_error: true,
    }
}

fn texts(content: &[ContentBlock]) -> String {
    content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// What an action did, then the window as it is now.
fn report(outcome: &ActionResult, vision: bool) -> Vec<ContentBlock> {
    let head = match (outcome.ok, outcome.refused) {
        (_, Some(refusal)) => format!(
            "Refused: {}. Nothing was done. Here is the window now.",
            serde_json::to_string(&refusal).unwrap_or_default()
        ),
        (false, None) => "The input couldn't be delivered. Here is the window now.".into(),
        (true, None) if outcome.verify_required => {
            "Input sent. That doesn't mean it worked: check the window below.".into()
        }
        (true, None) => "Done. Here is the window now.".into(),
    };
    let mut content = vec![ContentBlock::text(head)];
    content.extend(describe(&outcome.observation, vision));
    content
}

/// The observation as the model gets it. The token stays with the loop.
fn describe(observation: &Observation, vision: bool) -> Vec<ContentBlock> {
    let outline = serde_json::to_string(&observation.outline).unwrap_or_default();
    let mut content = vec![ContentBlock::text(format!(
        "Window \"{}\" ({}), {}x{} px. Outline: {outline}",
        observation.window.title,
        observation.window.app,
        observation.size.width,
        observation.size.height
    ))];
    if vision && let Some(image) = &observation.image {
        // Data URLs only for now; adapters decide how to send them.
        if let Some(base64) = image.strip_prefix("data:image/png;base64,") {
            content.push(ContentBlock::Image {
                media_type: "image/png".into(),
                base64: base64.into(),
            });
        }
    }
    content
}

fn now() -> String {
    humantime::format_rfc3339_seconds(SystemTime::now()).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use hands::fake::FakeSettings;
    use llm::MockProvider;
    use serde_json::json;

    fn task() -> Task {
        Task {
            prompt: "Turn on dark mode".into(),
            model: "scripted".into(),
            limits: Limits::default(),
        }
    }

    async fn play(
        provider: &dyn Provider,
        hands: &dyn Hands,
        task: &Task,
    ) -> (Outcome, Vec<RunEvent>) {
        play_until(provider, hands, task, &CancellationToken::new()).await
    }

    async fn play_until(
        provider: &dyn Provider,
        hands: &dyn Hands,
        task: &Task,
        stop: &CancellationToken,
    ) -> (Outcome, Vec<RunEvent>) {
        let mut events = Vec::new();
        let outcome = run(provider, hands, task, stop, &mut |event: RunEvent| {
            events.push(event)
        })
        .await;
        (outcome, events)
    }

    fn finished_as(events: &[RunEvent]) -> Option<(RunStatus, Option<&str>)> {
        match events.last() {
            Some(RunEvent::Finished { status, reply }) => Some((*status, reply.as_deref())),
            _ => None,
        }
    }

    fn step_urls(events: &[RunEvent]) -> Vec<Option<String>> {
        events
            .iter()
            .filter_map(|event| match event {
                RunEvent::Step { step } => Some(step.screenshot_url.clone()),
                _ => None,
            })
            .collect()
    }

    /// A model that never answers: a stop must not wait for it.
    struct Stuck;

    #[async_trait::async_trait]
    impl Provider for Stuck {
        fn caps(&self, _model: &str) -> llm::Caps {
            llm::Caps::default()
        }

        async fn turn(&self, _req: TurnRequest) -> anyhow::Result<llm::TurnResponse> {
            std::future::pending().await
        }
    }

    /// A model that presses Stop while answering, so the stop lands while its
    /// action is in flight.
    struct StopsWhileActing {
        stop: CancellationToken,
        inner: MockProvider,
    }

    #[async_trait::async_trait]
    impl Provider for StopsWhileActing {
        fn caps(&self, model: &str) -> llm::Caps {
            self.inner.caps(model)
        }

        async fn turn(&self, req: TurnRequest) -> anyhow::Result<llm::TurnResponse> {
            self.stop.cancel();
            self.inner.turn(req).await
        }
    }

    /// The fake window, with a screenshot in every observation.
    struct WithImage(FakeSettings);

    const IMAGE: &str = "data:image/png;base64,iVBORw0KGgo=";

    #[async_trait::async_trait]
    impl Hands for WithImage {
        async fn observe(&self) -> anyhow::Result<Observation> {
            let mut observation = self.0.observe().await?;
            observation.image = Some(IMAGE.into());
            Ok(observation)
        }

        async fn act(&self, token: &str, action: &proto::Action) -> anyhow::Result<ActionResult> {
            let mut result = self.0.act(token, action).await?;
            result.observation.image = Some(IMAGE.into());
            Ok(result)
        }
    }

    /// A sink that keeps screenshots, the way a backend does.
    #[derive(Default)]
    struct Keeper {
        events: Vec<RunEvent>,
        shots: Vec<(u32, String)>,
    }

    impl Sink for Keeper {
        fn event(&mut self, event: RunEvent) {
            self.events.push(event);
        }

        fn screenshot(&mut self, n: u32, image: &str) -> Option<String> {
            self.shots.push((n, image.to_owned()));
            Some(format!("/shots/{n}"))
        }
    }

    fn press(element: &str) -> serde_json::Value {
        json!({"do": "act", "element": element, "op": "click"})
    }

    #[tokio::test]
    async fn the_demo_turns_on_dark_mode() {
        let (provider, hands) = (MockProvider::new(demo::script()), FakeSettings::new());
        let (outcome, events) = play(&provider, &hands, &task()).await;
        assert_eq!(outcome.status, RunStatus::Succeeded);
        assert_eq!(outcome.steps, 2);
        // An independent check, not the model's word.
        assert!(hands.is_dark());
        assert!(matches!(
            events.first(),
            Some(RunEvent::Status {
                status: RunStatus::Running
            })
        ));
        assert!(matches!(
            events.last(),
            Some(RunEvent::Finished {
                status: RunStatus::Succeeded,
                ..
            })
        ));
        // The model never sees the token.
        let sent = format!("{:?}", provider.requests());
        assert!(!sent.contains("\"t1\"") && !sent.contains("\"t2\""));
        // The fake window has no screenshots, so the steps point nowhere.
        assert_eq!(step_urls(&events), [None, None]);
    }

    #[tokio::test]
    async fn a_stop_before_the_first_turn_asks_the_model_nothing() {
        let (provider, hands) = (MockProvider::new(demo::script()), FakeSettings::new());
        let stop = CancellationToken::new();
        stop.cancel();
        let (outcome, events) = play_until(&provider, &hands, &task(), &stop).await;
        assert_eq!(outcome.status, RunStatus::Stopped);
        assert_eq!(outcome.steps, 0);
        assert!(provider.requests().is_empty());
        assert_eq!(
            finished_as(&events),
            Some((RunStatus::Stopped, Some(STOPPED_BY_REQUEST)))
        );
    }

    #[tokio::test]
    async fn a_stop_does_not_wait_for_the_model() {
        let stop = CancellationToken::new();
        let later = stop.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            later.cancel();
        });
        let (outcome, events) = play_until(&Stuck, &FakeSettings::new(), &task(), &stop).await;
        assert_eq!(outcome.status, RunStatus::Stopped);
        assert_eq!(outcome.steps, 0);
        assert_eq!(
            finished_as(&events),
            Some((RunStatus::Stopped, Some(STOPPED_BY_REQUEST)))
        );
    }

    #[tokio::test]
    async fn a_stop_waits_for_the_action_in_flight() {
        let stop = CancellationToken::new();
        let provider = StopsWhileActing {
            stop: stop.clone(),
            inner: MockProvider::new(demo::script()),
        };
        let hands = FakeSettings::new();
        let (outcome, events) = play_until(&provider, &hands, &task(), &stop).await;
        // The first action still ran and was reported; the second turn never came.
        assert_eq!(outcome.status, RunStatus::Stopped);
        assert_eq!(outcome.steps, 1);
        assert_eq!(provider.inner.requests().len(), 1);
        assert!(matches!(
            events.iter().find(|event| matches!(event, RunEvent::Step { .. })),
            Some(RunEvent::Step { step }) if step.ok && step.n == 1
        ));
        assert!(!hands.is_dark(), "the second action never ran");
    }

    #[tokio::test]
    async fn screenshots_go_to_the_sink_and_their_urls_into_the_steps() {
        let provider = MockProvider::new(demo::script());
        let hands = WithImage(FakeSettings::new());
        let mut keeper = Keeper::default();
        let outcome = run(
            &provider,
            &hands,
            &task(),
            &CancellationToken::new(),
            &mut keeper,
        )
        .await;
        assert_eq!(outcome.status, RunStatus::Succeeded);
        assert_eq!(keeper.shots, [(1, IMAGE.to_owned()), (2, IMAGE.to_owned())]);
        assert_eq!(
            step_urls(&keeper.events),
            [Some("/shots/1".to_owned()), Some("/shots/2".to_owned())]
        );
    }

    #[tokio::test]
    async fn done_with_an_action_in_the_same_turn_is_refused() {
        let mut both = MockProvider::call("Opening Appearance.", "c1", "computer", press("e3"));
        both.extend(MockProvider::call(
            "",
            "c2",
            "done",
            json!({"reply": "done!"}),
        ));
        let provider = MockProvider::new([
            both,
            MockProvider::call(
                "Saw it open.",
                "c3",
                "done",
                json!({"reply": "Appearance is open."}),
            ),
        ]);
        let hands = FakeSettings::new();
        let (outcome, _) = play(&provider, &hands, &task()).await;
        assert_eq!(outcome.status, RunStatus::Succeeded);
        assert_eq!(outcome.reply.as_deref(), Some("Appearance is open."));
        assert_eq!(provider.requests().len(), 2, "done had to wait a turn");
    }

    #[tokio::test]
    async fn the_turn_budget_stops_the_run() {
        let provider = MockProvider::new(demo::script());
        let hands = FakeSettings::new();
        let task = Task {
            limits: Limits {
                max_turns: 1,
                ..Limits::default()
            },
            ..task()
        };
        let (outcome, _) = play(&provider, &hands, &task).await;
        assert_eq!(outcome.status, RunStatus::Stopped);
        assert_eq!(outcome.steps, 1);
    }

    #[tokio::test]
    async fn bad_calls_are_reported_to_the_model_not_retried() {
        let provider = MockProvider::new([
            MockProvider::call("", "c1", "computer", json!({"do": "teleport"})),
            MockProvider::call("", "c2", "done", json!({"reply": "gave up"})),
        ]);
        let hands = FakeSettings::new();
        let (outcome, _) = play(&provider, &hands, &task()).await;
        assert_eq!(outcome.steps, 0);
        let second = &provider.requests()[1];
        let last = second.messages.last().unwrap();
        assert!(matches!(
            &last.content[0],
            ContentBlock::ToolResult { is_error: true, .. }
        ));
    }

    #[tokio::test]
    async fn a_provider_error_fails_the_run() {
        let (provider, hands) = (MockProvider::new([]), FakeSettings::new());
        let (outcome, events) = play(&provider, &hands, &task()).await;
        assert_eq!(outcome.status, RunStatus::Failed);
        assert!(
            events
                .iter()
                .any(|event| matches!(event, RunEvent::Error { .. }))
        );
    }
}
