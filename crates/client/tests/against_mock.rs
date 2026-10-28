//! The client against the real routes and the mock backend, over real sockets.

use std::{future::IntoFuture, sync::Arc, time::Duration};

use api::{Backend, MockBackend};
use client::{Client, ClientError};
use futures::StreamExt;
use proto::{RunEvent, RunStatus, StartRun};

async fn serve_mock() -> Client {
    let backend: Arc<dyn Backend> = Arc::new(MockBackend::new(Duration::from_millis(5)));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(axum::serve(listener, api::router(backend)).into_future());
    Client::new(url)
}

fn start(prompt: &str) -> StartRun {
    StartRun {
        prompt: prompt.into(),
        provider: None,
        model: None,
        target: None,
        max_steps: None,
        max_cost_usd: None,
    }
}

#[tokio::test]
async fn reads_health_and_runs() {
    let client = serve_mock().await;
    assert!(client.health().await.unwrap().mock);
    assert_eq!(client.runs().await.unwrap().len(), 1, "the mock starts with a demo run");
}

#[tokio::test]
async fn follows_a_run_to_the_end() {
    let client = serve_mock().await;
    let run = client.start_run(&start("turn on dark mode")).await.unwrap();
    let events: Vec<RunEvent> = client
        .events(&run.id)
        .await
        .unwrap()
        .map(Result::unwrap)
        .collect()
        .await;
    let steps = events
        .iter()
        .filter(|event| matches!(event, RunEvent::Step { .. }))
        .count();
    assert_eq!(steps, 4);
    assert!(matches!(
        events.last(),
        Some(RunEvent::Finished {
            status: RunStatus::Succeeded,
            ..
        })
    ));
}

#[tokio::test]
async fn server_errors_keep_their_message() {
    let client = serve_mock().await;
    let error = client.start_run(&start("  ")).await.unwrap_err();
    let error = error.downcast::<ClientError>().unwrap();
    assert_eq!(error.status, 400);
    assert_eq!(error.message, "prompt is empty");
}

#[tokio::test]
async fn a_dead_server_is_an_error_not_a_hang() {
    let client = Client::new("http://127.0.0.1:9");
    assert!(client.health().await.is_err());
}
