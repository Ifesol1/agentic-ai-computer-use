//! `harness`: the club's computer-use agent.

use std::{net::SocketAddr, sync::Arc, time::Duration};

use anyhow::{Context, bail};
use clap::{Parser, Subcommand};
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(version, about = "The club's computer-use agent")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Serve the HTTP + WebSocket API the app uses.
    Serve {
        #[arg(long, default_value = "127.0.0.1:7878")]
        addr: SocketAddr,
        /// Play scripted runs: no keys, no models, no desktop.
        #[arg(long)]
        mock: bool,
        /// Pause before each mock step, in milliseconds.
        #[arg(long, default_value_t = 900)]
        mock_step_ms: u64,
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
        } => serve(addr, mock, Duration::from_millis(mock_step_ms)).await,
        Command::Run { .. } => {
            bail!("`harness run` isn't built yet (the agent loop); try `harness serve --mock`")
        }
    }
}

async fn serve(addr: SocketAddr, mock: bool, step_delay: Duration) -> anyhow::Result<()> {
    if !mock {
        bail!("only the mock backend exists so far: run `harness serve --mock`");
    }
    let backend: Arc<dyn api::Backend> = Arc::new(api::MockBackend::new(step_delay));
    let app = api::router(backend).layer(TraceLayer::new_for_http());
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("can't listen on {addr}"))?;
    tracing::info!("mock API on http://{addr}  (try GET /api/runs)");
    axum::serve(listener, app).await?;
    Ok(())
}
