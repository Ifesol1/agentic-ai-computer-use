//! Network calls run on a small tokio runtime next to GPUI's own executor:
//! reqwest and the WebSocket client need tokio, and GPUI can await their results.
//!
//! Which backend: `HARNESS_URL` when it's set (a `harness serve`, the club server),
//! else the scripted mock, started inside the app. Either way it's the same
//! HTTP + WebSocket API, so the views can't tell the difference.

use std::{future::Future, sync::Arc, time::Duration};

use client::Client;
use gpui_kit::{App, Global};
use tokio::{runtime::Runtime, task::AbortHandle};

/// Reach it from any view with `Net::get(cx)`.
#[derive(Clone)]
pub struct Net {
    runtime: Arc<Runtime>,
    client: Client,
}

impl Global for Net {}

impl Net {
    /// The backend `HARNESS_URL` names, or the built-in mock.
    pub fn start() -> Self {
        Self::connect(backend_url(std::env::var("HARNESS_URL").ok()))
    }

    /// `url`'s backend, or the built-in mock when it's `None`.
    fn connect(url: Option<String>) -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("net")
            .enable_all()
            .build()
            .expect("the network runtime starts");
        let client = match url {
            Some(url) => {
                let _inside = runtime.enter();
                Client::new(url)
            }
            None => runtime
                .block_on(Client::spawn_mock(Duration::from_millis(900)))
                .expect("the built-in mock backend starts"),
        };
        Self {
            runtime: Arc::new(runtime),
            client,
        }
    }

    pub fn get(cx: &App) -> Self {
        cx.global::<Self>().clone()
    }

    pub fn url(&self) -> &str {
        self.client.base_url()
    }

    /// Runs one API call on the network runtime. Await the result from GPUI:
    ///
    /// ```ignore
    /// let runs = Net::get(cx).call(|api| async move { api.runs().await });
    /// cx.spawn(async move |this, cx| { let runs = runs.await; /* this.update(cx, ...) */ })
    /// ```
    pub fn call<F, Fut, T>(
        &self,
        call: F,
    ) -> impl Future<Output = anyhow::Result<T>> + use<F, Fut, T>
    where
        F: FnOnce(Client) -> Fut,
        Fut: Future<Output = anyhow::Result<T>> + Send + 'static,
        T: Send + 'static,
    {
        let job = self.runtime.spawn(call(self.client.clone()));
        async move { job.await? }
    }

    /// Starts a long-lived network task, such as following a run's events.
    pub fn spawn(&self, task: impl Future<Output = ()> + Send + 'static) -> AbortHandle {
        self.runtime.spawn(task).abort_handle()
    }

    pub fn client(&self) -> Client {
        self.client.clone()
    }
}

/// `HARNESS_URL`'s value, if it names a backend (an empty one doesn't).
fn backend_url(var: Option<String>) -> Option<String> {
    var.map(|url| url.trim().to_owned())
        .filter(|url| !url.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{channel::oneshot, executor::block_on};

    #[test]
    fn harness_url_picks_the_backend() {
        assert_eq!(backend_url(None), None);
        assert_eq!(backend_url(Some("  ".into())), None);
        assert_eq!(
            backend_url(Some(" http://10.0.0.2:7878 ".into())).as_deref(),
            Some("http://10.0.0.2:7878")
        );
    }

    #[test]
    fn calls_reach_the_built_in_mock() {
        let net = Net::connect(None);
        assert!(net.url().starts_with("http://127.0.0.1:"), "{}", net.url());
        // Awaited off the network runtime, as GPUI's executor does.
        let health = block_on(net.call(|api| async move { api.health().await })).unwrap();
        assert!(health.mock);
        let runs = block_on(net.call(|api| async move { api.runs().await })).unwrap();
        assert!(!runs.is_empty(), "the mock starts with a demo run");
    }

    #[test]
    fn spawned_tasks_run_on_the_network_runtime() {
        let net = Net::connect(None);
        let (done, finished) = oneshot::channel();
        // tokio's timer only works on a tokio runtime, so this proves where it ran.
        net.spawn(async move {
            tokio::time::sleep(Duration::from_millis(1)).await;
            let _ = done.send(());
        });
        block_on(finished).unwrap();
    }

    #[test]
    fn a_dead_backend_is_an_error() {
        let net = Net::connect(Some("http://127.0.0.1:9".into()));
        assert!(block_on(net.call(|api| async move { api.health().await })).is_err());
    }
}
