//! `harness`: the club's computer-use agent.

use std::{net::SocketAddr, sync::Arc, time::Duration};

use anyhow::{Context, bail};
use axum::http::HeaderValue;
use clap::{Parser, Subcommand};
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(version, about = "The club's computer-use agent")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Serve the HTTP + WebSocket API the frontend uses.
    Serve {
        #[arg(long, default_value = "127.0.0.1:7878")]
        addr: SocketAddr,
        /// Play scripted runs: no keys, no models, no desktop.
        #[arg(long)]
        mock: bool,
        /// Pause before each mock step, in milliseconds.
        #[arg(long, default_value_t = 900)]
        mock_step_ms: u64,
        /// Let this browser origin call the API directly, e.g. http://localhost:5173.
        /// A dev-server proxy is simpler; repeat the flag for more origins.
        #[arg(long)]
        allow_origin: Vec<String>,
    },
    /// Run one task from the terminal.
    Run { prompt: String },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();
    match Cli::parse().command {
        Command::Serve {
            addr,
            mock,
            mock_step_ms,
            allow_origin,
        } => {
            serve(
                addr,
                mock,
                Duration::from_millis(mock_step_ms),
                allow_origin,
            )
            .await
        }
        Command::Run { .. } => {
            bail!("`harness run` isn't built yet (the agent loop); try `harness serve --mock`")
        }
    }
}

async fn serve(
    addr: SocketAddr,
    mock: bool,
    step_delay: Duration,
    allow_origin: Vec<String>,
) -> anyhow::Result<()> {
    if !mock {
        bail!("only the mock backend exists so far: run `harness serve --mock`");
    }
    let backend: Arc<dyn api::Backend> = Arc::new(api::MockBackend::new(step_delay));
    let mut app = api::router(backend).layer(TraceLayer::new_for_http());
    if !allow_origin.is_empty() {
        let origins = allow_origin
            .iter()
            .map(|origin| origin.parse::<HeaderValue>())
            .collect::<Result<Vec<_>, _>>()
            .context("--allow-origin")?;
        app = app.layer(
            CorsLayer::new()
                .allow_origin(origins)
                .allow_methods(Any)
                .allow_headers(Any),
        );
    }
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("can't listen on {addr}"))?;
    tracing::info!("mock API on http://{addr}  (try GET /api/runs)");
    axum::serve(listener, app).await?;
    Ok(())
}
