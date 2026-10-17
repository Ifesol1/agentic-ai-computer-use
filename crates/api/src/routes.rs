use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{
        Path, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use proto::{ApiError, Health, RunEvent, StartRun};
use tokio::sync::broadcast::{self, error::RecvError};

use crate::{Backend, BadRequest};

type Shared = Arc<dyn Backend>;

/// Every route of the API. Mount it as is; add CORS or tracing layers around it.
pub fn router(backend: Arc<dyn Backend>) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/providers", get(providers))
        .route("/api/runs", get(list_runs).post(start_run))
        .route("/api/runs/{id}", get(get_run))
        .route("/api/runs/{id}/stop", post(stop_run))
        .route("/api/runs/{id}/events", get(events))
        .route("/api/runs/{id}/steps/{n}/screenshot", get(screenshot))
        .with_state(backend)
}

fn error(status: StatusCode, message: impl Into<String>) -> Response {
    let body = ApiError {
        error: message.into(),
    };
    (status, Json(body)).into_response()
}

fn no_run(id: &str) -> Response {
    error(StatusCode::NOT_FOUND, format!("no run {id}"))
}

async fn health(State(backend): State<Shared>) -> Json<Health> {
    Json(Health {
        version: env!("CARGO_PKG_VERSION").into(),
        contract: proto::CONTRACT_VERSION,
        mock: backend.is_mock(),
    })
}

async fn providers(State(backend): State<Shared>) -> Response {
    Json(backend.providers().await).into_response()
}

async fn list_runs(State(backend): State<Shared>) -> Response {
    Json(backend.list_runs().await).into_response()
}

async fn start_run(State(backend): State<Shared>, Json(req): Json<StartRun>) -> Response {
    match backend.start_run(req).await {
        Ok(run) => (StatusCode::CREATED, Json(run)).into_response(),
        Err(BadRequest(message)) => error(StatusCode::BAD_REQUEST, message),
    }
}

async fn get_run(State(backend): State<Shared>, Path(id): Path<String>) -> Response {
    match backend.get_run(&id).await {
        Some(run) => Json(run).into_response(),
        None => no_run(&id),
    }
}

async fn stop_run(State(backend): State<Shared>, Path(id): Path<String>) -> Response {
    match backend.stop_run(&id).await {
        Some(run) => Json(run).into_response(),
        None => no_run(&id),
    }
}

async fn screenshot(State(backend): State<Shared>, Path((id, n)): Path<(String, u32)>) -> Response {
    match backend.screenshot(&id, n).await {
        Some((content_type, bytes)) => {
            ([(header::CONTENT_TYPE, content_type)], bytes).into_response()
        }
        None => error(
            StatusCode::NOT_FOUND,
            format!("no screenshot {n} in run {id}"),
        ),
    }
}

async fn events(
    State(backend): State<Shared>,
    Path(id): Path<String>,
    ws: WebSocketUpgrade,
) -> Response {
    let Some((history, live)) = backend.subscribe(&id).await else {
        return no_run(&id);
    };
    ws.on_upgrade(move |socket| stream(socket, history, live))
}

/// Replays the history, then forwards live events until the run finishes.
async fn stream(
    mut socket: WebSocket,
    history: Vec<RunEvent>,
    mut live: broadcast::Receiver<RunEvent>,
) {
    for event in &history {
        if send(&mut socket, event).await.is_err() {
            return;
        }
    }
    loop {
        match live.recv().await {
            Ok(event) => {
                if send(&mut socket, &event).await.is_err() {
                    return;
                }
            }
            // A slow client fell behind: close, and it replays from the start on reconnect.
            Err(RecvError::Lagged(missed)) => {
                tracing::warn!(missed, "event stream lagged; closing so the client replays");
                break;
            }
            Err(RecvError::Closed) => break,
        }
    }
    let _ = socket.send(Message::Close(None)).await;
}

async fn send(socket: &mut WebSocket, event: &RunEvent) -> Result<(), axum::Error> {
    let json = serde_json::to_string(event).expect("RunEvent always serializes");
    socket.send(Message::Text(json.into())).await
}
