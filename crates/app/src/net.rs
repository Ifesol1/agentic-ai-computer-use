//! Network calls run on a small tokio runtime next to GPUI's own executor:
//! reqwest and the WebSocket client need tokio, and GPUI can await their results.

use std::{future::Future, sync::Arc};

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
    pub fn start() -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("net")
            .enable_all()
            .build()
            .expect("the network runtime starts");
        let client = {
            let _inside = runtime.enter();
            Client::from_env()
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
    pub fn call<F, Fut, T>(&self, call: F) -> impl Future<Output = anyhow::Result<T>> + use<F, Fut, T>
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
