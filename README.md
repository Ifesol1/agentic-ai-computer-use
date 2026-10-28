# agentic-ai-computer-use

Agentic AI Club - PC sub team: computer-use agent project

The club's own agent harness: one agent loop that can see and drive a computer, with the model as a setting, and a desktop app (GPUI) to start runs and watch them live.

## Quick start

Install Rust from https://rustup.rs (the pinned version installs itself on the first build), then:

```bash
git clone https://github.com/Ifesol1/agentic-ai-computer-use.git
cd agentic-ai-computer-use
cargo test     # builds everything and runs the tests
cargo dev      # starts the mock backend and opens the app
```

Linux also needs a few system libraries for GPUI; macOS and Windows have one-time notes too. See [CONTRIBUTING.md](CONTRIBUTING.md#setup-once).

## Frontend and backend at the same time

The app and the backend are separate programs that share one set of Rust types (`crates/proto`), so each side can run, restart and change on its own:

| You work on | Run | Use |
|---|---|---|
| The app | `cargo dev` | The mock backend plays scripted runs: no keys, no model, no desktop |
| The backend | `cargo run -p agent -- serve --mock` | `cargo run -p app` in a second terminal to see it, or the tests |
| Both | Both commands, in two terminals | The app reconnects whenever the backend restarts |

`cargo dev` uses a backend that's already running on 127.0.0.1:7878 instead of starting its own.

| Folder | What's there |
|---|---|
| `crates/proto` | The contract: types shared by the app, server, agent loop and devices |
| `crates/api` | HTTP + WebSocket routes (axum) and the mock backend |
| `crates/client` | A typed client for the API; the app talks to the backend only through it |
| `crates/app` | The desktop app (GPUI, with GPUI Kit components), one file per view |
| `crates/agent` | The `harness` binary: `serve --mock` today, the agent loop next |
| `crates/llm` | Model providers behind one trait |
| `crates/hands` | Screens and input behind one trait |
| `crates/device` | The agent that runs on a member's PC (not built yet) |
| `crates/xtask` | `cargo dev` |

Read [CONTRIBUTING.md](CONTRIBUTING.md) before your first PR.
