//! `harness`: the club's computer-use agent.

use std::{
    fs::{self, File},
    io::{BufWriter, Write},
    net::SocketAddr,
    path::PathBuf,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use agent::{Limits, Task};
use anyhow::{Context, bail};
use clap::{Parser, Subcommand};
use proto::{RunEvent, RunStatus};
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
    /// Run one task from the terminal with the real agent loop.
    Run {
        prompt: String,
        /// The scripted model on the fake Settings window: no keys, no desktop.
        #[arg(long)]
        mock: bool,
        /// Adapter kind (see `harness providers`). Ignored with --mock.
        #[arg(short, long, default_value = "anthropic")]
        provider: String,
        #[arg(short, long, default_value = "")]
        model: String,
        /// The window to borrow (window mode). Ignored with --mock.
        #[arg(long, default_value = "")]
        window: String,
        #[arg(long, default_value_t = Limits::default().max_turns)]
        max_steps: u32,
        #[arg(long, default_value_t = Limits::default().max_cost_usd)]
        max_cost: f64,
        /// Every event is appended to <log-dir>/<run id>.jsonl, failures too.
        #[arg(long, default_value = "runs")]
        log_dir: PathBuf,
    },
    /// The adapter kinds this build knows.
    Providers,
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
        Command::Run {
            prompt,
            mock,
            provider,
            model,
            window,
            max_steps,
            max_cost,
            log_dir,
        } => {
            let task = Task {
                prompt,
                model: if mock { "scripted".into() } else { model },
                limits: Limits {
                    max_turns: max_steps,
                    max_cost_usd: max_cost,
                },
            };
            let (provider, hands): (Box<dyn llm::Provider>, Box<dyn hands::Hands>) = if mock {
                (
                    Box::new(llm::MockProvider::new(agent::demo::script())),
                    Box::new(hands::fake::FakeSettings::new()),
                )
            } else {
                let config = llm::ProviderConfig {
                    name: provider.clone(),
                    kind: provider,
                    model: task.model.clone(),
                    ..Default::default()
                };
                (
                    llm::build(&config)?,
                    Box::new(hands::WindowMode::new(window)),
                )
            };
            run(provider.as_ref(), hands.as_ref(), &task, &log_dir).await
        }
        Command::Providers => {
            println!("mock (harness run --mock)");
            for kind in llm::kinds() {
                println!("{kind}");
            }
            Ok(())
        }
    }
}

async fn run(
    provider: &dyn llm::Provider,
    hands: &dyn hands::Hands,
    task: &Task,
    log_dir: &PathBuf,
) -> anyhow::Result<()> {
    let id = format!(
        "run-{}",
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis()
    );
    fs::create_dir_all(log_dir).with_context(|| format!("can't make {}", log_dir.display()))?;
    let path = log_dir.join(format!("{id}.jsonl"));
    let mut log = BufWriter::new(File::create(&path)?);
    let outcome = agent::run(provider, hands, task, &mut |event: RunEvent| {
        print_event(&event);
        if let Ok(line) = serde_json::to_string(&event) {
            let _ = writeln!(log, "{line}");
        }
    })
    .await;
    log.flush()?;
    println!(
        "{} steps · {} tokens · ${:.4} · log: {}",
        outcome.steps,
        outcome.usage.input_tokens + outcome.usage.output_tokens,
        outcome.usage.cost_usd,
        path.display()
    );
    if outcome.status != RunStatus::Succeeded {
        bail!("the run ended {:?}", outcome.status);
    }
    Ok(())
}

fn print_event(event: &RunEvent) {
    match event {
        RunEvent::Step { step } => {
            let action = serde_json::to_string(&step.action).unwrap_or_default();
            let refused = step
                .refused
                .map(|refusal| format!("  refused: {refusal:?}"))
                .unwrap_or_default();
            println!("{:>3}. {}\n     {action}{refused}", step.n, step.note);
        }
        RunEvent::Finished { status, reply } => {
            println!("{status:?}: {}", reply.as_deref().unwrap_or(""));
        }
        RunEvent::Error { message } => eprintln!("error: {message}"),
        RunEvent::Status { .. } | RunEvent::Usage { .. } => {}
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
