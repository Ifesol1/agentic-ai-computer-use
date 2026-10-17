//! The API as the frontend sees it. A real backend must pass these too.

use std::{future::IntoFuture, sync::Arc, time::Duration};

use api::{Backend, MockBackend};
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use futures_util::StreamExt;
use proto::{ApiError, Health, ProviderInfo, RunEvent, RunInfo, RunStatus};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;
use tower::ServiceExt;

fn app(step_ms: u64) -> Router {
    let backend: Arc<dyn Backend> = Arc::new(MockBackend::new(Duration::from_millis(step_ms)));
    api::router(backend)
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

#[tokio::test]
async fn health_says_mock() {
    let (status, body) = call(&app(0), "GET", "/api/health", None).await;
    assert_eq!(status, StatusCode::OK);
    let health: Health = serde_json::from_value(body).unwrap();
    assert!(health.mock);
    assert_eq!(health.contract, proto::CONTRACT_VERSION);
}

#[tokio::test]
async fn exactly_one_default_provider() {
    let (_, body) = call(&app(0), "GET", "/api/providers", None).await;
    let providers: Vec<ProviderInfo> = serde_json::from_value(body).unwrap();
    assert_eq!(providers.iter().filter(|p| p.is_default).count(), 1);
}

#[tokio::test]
async fn a_started_run_is_listed_first() {
    let app = app(0);
    let (status, body) = call(
        &app,
        "POST",
        "/api/runs",
        Some(json!({"prompt": "turn on dark mode"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let run: RunInfo = serde_json::from_value(body).unwrap();

    let (_, body) = call(&app, "GET", "/api/runs", None).await;
    let runs: Vec<RunInfo> = serde_json::from_value(body).unwrap();
    assert_eq!(runs[0].id, run.id);

    let (status, _) = call(&app, "GET", &format!("/api/runs/{}", run.id), None).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn errors_are_json() {
    let app = app(0);
    let (status, body) = call(&app, "GET", "/api/runs/nope", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    serde_json::from_value::<ApiError>(body).unwrap();

    let bad = json!({"prompt": "x", "provider": "nope"});
    let (status, body) = call(&app, "POST", "/api/runs", Some(bad)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    serde_json::from_value::<ApiError>(body).unwrap();
}

#[tokio::test]
async fn stop_ends_a_run() {
    let app = app(60_000);
    let (_, body) = call(&app, "POST", "/api/runs", Some(json!({"prompt": "slow"}))).await;
    let run: RunInfo = serde_json::from_value(body).unwrap();
    let (status, body) = call(&app, "POST", &format!("/api/runs/{}/stop", run.id), None).await;
    assert_eq!(status, StatusCode::OK);
    let run: RunInfo = serde_json::from_value(body).unwrap();
    assert_eq!(run.status, RunStatus::Stopped);
}

/// Over a real socket: history, then live events, then the socket closes.
#[tokio::test]
async fn events_stream_until_finished() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = app(5);
    tokio::spawn(axum::serve(listener, app.clone()).into_future());

    let (_, body) = call(
        &app,
        "POST",
        "/api/runs",
        Some(json!({"prompt": "turn on dark mode"})),
    )
    .await;
    let run: RunInfo = serde_json::from_value(body).unwrap();

    let url = format!("ws://{addr}/api/runs/{}/events", run.id);
    let (mut socket, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    let mut steps = Vec::new();
    let mut finished = None;
    while let Some(frame) = socket.next().await {
        match frame.unwrap() {
            Message::Text(text) => match serde_json::from_str::<RunEvent>(&text).unwrap() {
                RunEvent::Step { step } => steps.push(step.n),
                RunEvent::Finished { status, .. } => finished = Some(status),
                _ => {}
            },
            Message::Close(_) => break,
            _ => {}
        }
    }
    assert_eq!(steps, [1, 2, 3, 4]);
    assert_eq!(finished, Some(RunStatus::Succeeded));

    let shot = format!("/api/runs/{}/steps/3/screenshot", run.id);
    let response = app
        .oneshot(Request::get(shot).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}
