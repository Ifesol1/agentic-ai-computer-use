//! A typed client for the harness API. The app talks to the backend only through
//! this, so the backend can be the mock, a laptop's `harness serve`, or the club
//! server without the app changing.

use std::time::Duration;

use anyhow::{Context, anyhow};
use futures::{Stream, StreamExt};
use proto::{ApiError, Health, ProviderInfo, RunEvent, RunInfo, StartRun};
use reqwest::{Response, StatusCode};
use serde::de::DeserializeOwned;
use tokio_tungstenite::tungstenite::Message;

/// Where `harness serve` listens unless told otherwise.
pub const DEFAULT_URL: &str = "http://127.0.0.1:7878";

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    base: String,
}

impl Client {
    pub fn new(base_url: impl Into<String>) -> Self {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(15))
            .build()
            .expect("the HTTP client builds with these settings");
        Self {
            http,
            base: base_url.into().trim_end_matches('/').to_owned(),
        }
    }

    /// `HARNESS_URL`, or [`DEFAULT_URL`].
    pub fn from_env() -> Self {
        Self::new(std::env::var("HARNESS_URL").unwrap_or_else(|_| DEFAULT_URL.to_owned()))
    }

    pub fn base_url(&self) -> &str {
        &self.base
    }

    pub async fn health(&self) -> anyhow::Result<Health> {
        self.get("/api/health").await
    }

    pub async fn providers(&self) -> anyhow::Result<Vec<ProviderInfo>> {
        self.get("/api/providers").await
    }

    /// Newest first.
    pub async fn runs(&self) -> anyhow::Result<Vec<RunInfo>> {
        self.get("/api/runs").await
    }

    pub async fn run(&self, id: &str) -> anyhow::Result<RunInfo> {
        self.get(&format!("/api/runs/{id}")).await
    }

    pub async fn start_run(&self, req: &StartRun) -> anyhow::Result<RunInfo> {
        let response = self.http.post(self.url("/api/runs")).json(req).send().await;
        read(response).await
    }

    pub async fn stop_run(&self, id: &str) -> anyhow::Result<RunInfo> {
        let path = format!("/api/runs/{id}/stop");
        read(self.http.post(self.url(&path)).send().await).await
    }

    /// A run's events: everything so far, then live ones. The stream ends after
    /// `finished`, or when the connection drops (open it again to replay).
    pub async fn events(
        &self,
        id: &str,
    ) -> anyhow::Result<impl Stream<Item = anyhow::Result<RunEvent>> + Send + 'static> {
        let url = self.url(&format!("/api/runs/{id}/events"));
        let url = url.replacen("http", "ws", 1);
        let (socket, _) = tokio_tungstenite::connect_async(url.as_str())
            .await
            .with_context(|| format!("can't open {url}"))?;
        Ok(socket.filter_map(|frame| async move {
            match frame {
                Ok(Message::Text(text)) => Some(serde_json::from_str(&text).map_err(Into::into)),
                Ok(_) => None,
                Err(error) => Some(Err(error.into())),
            }
        }))
    }

    async fn get<T: DeserializeOwned>(&self, path: &str) -> anyhow::Result<T> {
        read(self.http.get(self.url(path)).send().await).await
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }
}

/// The body as `T`, or the server's `ApiError` message as the error.
async fn read<T: DeserializeOwned>(response: reqwest::Result<Response>) -> anyhow::Result<T> {
    let response = response.context("the server isn't answering")?;
    let status = response.status();
    if status.is_success() {
        return response.json().await.context("unexpected answer from the server");
    }
    let message = match response.json::<ApiError>().await {
        Ok(body) => body.error,
        Err(_) => status.canonical_reason().unwrap_or("error").to_owned(),
    };
    Err(anyhow!(ClientError { status, message }))
}

/// A 4xx or 5xx answer.
#[derive(Debug)]
pub struct ClientError {
    pub status: StatusCode,
    pub message: String,
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.message, self.status.as_u16())
    }
}

impl std::error::Error for ClientError {}
