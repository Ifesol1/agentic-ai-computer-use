//! `AgentBackend` behind the real routes: the same API the frontend sees from
//! the mock, now with the real loop underneath. Mirrors `crates/api/tests/contract.rs`.

use std::{future::IntoFuture, sync::Arc};

use agent::{AgentBackend, ProviderSpec};
use async_trait::async_trait;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use futures_util::StreamExt;
use hands::{Hands, fake::FakeSettings};
use llm::{Caps, MockProvider, Provider, TurnRequest, TurnResponse};
use proto::{
    Action, ActionResult, ApiError, Health, Observation, ProviderInfo, RunEvent, RunInfo, RunStatus,
};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;
use tower::ServiceExt;

const IMAGE: &str = "data:image/png;base64,aGVsbG8=";

fn mock_info() -> ProviderInfo {
    ProviderInfo {
        name: "mock".into(),
        kind: "mock".into(),
        model: "scripted".into(),
        vision: true,
        native_computer: false,
        is_default: true,
    }
}

/// A model that never answers, so a run stays running until it's stopped.
struct Stuck;

#[async_trait]
impl Provider for Stuck {
    fn caps(&self, _model: &str) -> Caps {
        Caps::default()
    }

    async fn turn(&self, _req: TurnRequest) -> anyhow::Result<TurnResponse> {
        std::future::pending().await
    }
}

/// The fake window with a screenshot in every observation.
struct WithImage(FakeSettings);

#[async_trait]
impl Hands for WithImage {
    async fn observe(&self) -> anyhow::Result<Observation> {
        let mut observation = self.0.observe().await?;
        observation.image = Some(IMAGE.into());
        Ok(observation)
    }

    async fn act(&self, token: &str, action: &Action) -> anyhow::Result<ActionResult> {
        let mut result = self.0.act(token, action).await?;
        result.observation.image = Some(IMAGE.into());
        Ok(result)
    }
}

fn demo() -> Router {
    api::router(Arc::new(AgentBackend::demo()))
}

fn stuck() -> Router {
    let backend = AgentBackend::new(
        vec![ProviderSpec {
            info: mock_info(),
            build: Arc::new(|| Ok(Box::new(Stuck))),
        }],
        Arc::new(|_| Ok(Box::new(FakeSettings::new()))),
    );
    api::router(Arc::new(backend))
}

fn with_images() -> Router {
    let backend = AgentBackend::new(
        vec![ProviderSpec {
            info: mock_info(),
            build: Arc::new(|| Ok(Box::new(MockProvider::new(agent::demo::script())))),
        }],
        Arc::new(|_| Ok(Box::new(WithImage(FakeSettings::new())))),
    );
    api::router(Arc::new(backend))
}

async fn call(app: &Router, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let request = Request::builder().method(method).uri(uri);
    let request = match body {
        Some(body) => request
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string())),
        None => request.body(Body::empty()),
    };
    let response = app.clone().oneshot(request.unwrap()).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn start(app: &Router, body: Value) -> RunInfo {
    let (status, body) = call(app, "POST", "/api/runs", Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    serde_json::from_value(body).unwrap()
}

/// Every event of a run over a real socket, until the server closes it.
async fn follow(app: &Router, id: &str) -> Vec<RunEvent> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(axum::serve(listener, app.clone()).into_future());
    let url = format!("ws://{addr}/api/runs/{id}/events");
    let (mut socket, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    let mut events = Vec::new();
    while let Some(frame) = socket.next().await {
        match frame {
            Ok(Message::Text(text)) => events.push(serde_json::from_str(&text).unwrap()),
            Ok(Message::Close(_)) | Err(_) => break,
            Ok(_) => {}
        }
    }
    events
}

#[tokio::test]
async fn health_is_not_mock_and_there_is_one_default_provider() {
    let app = demo();
    let (_, body) = call(&app, "GET", "/api/health", None).await;
    let health: Health = serde_json::from_value(body).unwrap();
    assert!(!health.mock);
    assert_eq!(health.contract, proto::CONTRACT_VERSION);

    let (_, body) = call(&app, "GET", "/api/providers", None).await;
    let providers: Vec<ProviderInfo> = serde_json::from_value(body).unwrap();
    assert_eq!(providers.iter().filter(|p| p.is_default).count(), 1);
}

#[tokio::test]
async fn a_run_goes_through_the_real_loop_to_the_end() {
    let app = demo();
    let run = start(&app, json!({"prompt": "turn on dark mode"})).await;
    assert_eq!(run.status, RunStatus::Queued);
    assert_eq!(run.provider, "mock");
    assert_eq!(run.model, "scripted");

    let events = follow(&app, &run.id).await;
    assert!(matches!(
        events.first(),
        Some(RunEvent::Status {
            status: RunStatus::Running
        })
    ));
    let steps: Vec<u32> = events
        .iter()
        .filter_map(|event| match event {
            RunEvent::Step { step } => Some(step.n),
            _ => None,
        })
        .collect();
    assert_eq!(
        steps,
        [1, 2],
        "the demo script: open Appearance, flip the switch"
    );
    assert!(matches!(
        events.last(),
        Some(RunEvent::Finished {
            status: RunStatus::Succeeded,
            reply: Some(_)
        })
    ));

    let (_, body) = call(&app, "GET", &format!("/api/runs/{}", run.id), None).await;
    let run: RunInfo = serde_json::from_value(body).unwrap();
    assert_eq!(run.status, RunStatus::Succeeded);
    assert_eq!(run.steps, 2);
    assert!(run.usage.input_tokens > 0 && run.usage.cost_usd > 0.0);
    assert!(run.reply.unwrap().contains("Dark mode is on"));

    // Listed newest first; a second subscriber replays the history and closes.
    let later = start(&app, json!({"prompt": "again"})).await;
    let (_, body) = call(&app, "GET", "/api/runs", None).await;
    let runs: Vec<RunInfo> = serde_json::from_value(body).unwrap();
    assert_eq!(runs[0].id, later.id);
    let replay = follow(&app, &run.id).await;
    assert_eq!(replay, events);
}

#[tokio::test]
async fn stop_ends_a_running_run() {
    let app = stuck();
    let run = start(&app, json!({"prompt": "wait forever"})).await;
    let (status, body) = call(&app, "POST", &format!("/api/runs/{}/stop", run.id), None).await;
    assert_eq!(status, StatusCode::OK);
    let run: RunInfo = serde_json::from_value(body).unwrap();
    assert_eq!(run.status, RunStatus::Stopped);
    assert_eq!(run.reply.as_deref(), Some(agent::STOPPED_BY_REQUEST));
    assert_eq!(run.steps, 0);

    // Stopping again changes nothing; the stream is the history, then closes.
    let (status, _) = call(&app, "POST", &format!("/api/runs/{}/stop", run.id), None).await;
    assert_eq!(status, StatusCode::OK);
    let events = follow(&app, &run.id).await;
    assert!(matches!(
        events.last(),
        Some(RunEvent::Finished {
            status: RunStatus::Stopped,
            ..
        })
    ));
}

#[tokio::test]
async fn screenshots_are_kept_per_step_and_served() {
    let app = with_images();
    let run = start(&app, json!({"prompt": "turn on dark mode"})).await;
    let events = follow(&app, &run.id).await;
    let urls: Vec<String> = events
        .iter()
        .filter_map(|event| match event {
            RunEvent::Step { step } => step.screenshot_url.clone(),
            _ => None,
        })
        .collect();
    assert_eq!(
        urls,
        [
            format!("/api/runs/{}/steps/1/screenshot", run.id),
            format!("/api/runs/{}/steps/2/screenshot", run.id),
        ]
    );

    let response = app
        .clone()
        .oneshot(Request::get(&urls[1]).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "image/png");
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(&bytes[..], b"hello");

    for n in [0, 3] {
        let (status, body) = call(
            &app,
            "GET",
            &format!("/api/runs/{}/steps/{n}/screenshot", run.id),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        serde_json::from_value::<ApiError>(body).unwrap();
    }
}

#[tokio::test]
async fn the_demo_window_has_no_screenshots() {
    let app = demo();
    let run = start(&app, json!({"prompt": "turn on dark mode"})).await;
    let events = follow(&app, &run.id).await;
    assert!(events.iter().all(|event| match event {
        RunEvent::Step { step } => step.screenshot_url.is_none(),
        _ => true,
    }));
}

#[tokio::test]
async fn bad_requests_are_refused_as_json() {
    let app = demo();
    for body in [
        json!({"prompt": "   "}),
        json!({"prompt": "x", "max_steps": 0}),
        json!({"prompt": "x", "provider": "nope"}),
    ] {
        let (status, answer) = call(&app, "POST", "/api/runs", Some(body.clone())).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        serde_json::from_value::<ApiError>(answer).unwrap();
    }
    let (status, _) = call(&app, "GET", "/api/runs/nope", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(&app, "POST", "/api/runs/nope/stop", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn budgets_from_the_request_reach_the_loop() {
    let app = demo();
    let run = start(&app, json!({"prompt": "turn on dark mode", "max_steps": 1})).await;
    let events = follow(&app, &run.id).await;
    assert!(matches!(
        events.last(),
        Some(RunEvent::Finished {
            status: RunStatus::Stopped,
            reply: Some(reply)
        }) if reply.contains("budget")
    ));
}

#[tokio::test]
async fn a_provider_that_cannot_be_built_fails_the_run() {
    let backend = AgentBackend::new(
        vec![ProviderSpec {
            info: mock_info(),
            build: Arc::new(|| anyhow::bail!("no API key for this provider")),
        }],
        Arc::new(|_| Ok(Box::new(FakeSettings::new()))),
    );
    let app = api::router(Arc::new(backend));
    let run = start(&app, json!({"prompt": "x"})).await;
    let events = follow(&app, &run.id).await;
    assert!(matches!(
        &events[0],
        RunEvent::Error { message } if message.contains("no API key")
    ));
    assert!(matches!(
        events.last(),
        Some(RunEvent::Finished {
            status: RunStatus::Failed,
            ..
        })
    ));
}
