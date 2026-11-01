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
cargo run -p agent -- run --mock "turn on dark mode"   # the real agent loop on a fake window
```

Linux also needs a few system libraries for GPUI; macOS and Windows have one-time notes too. See [CONTRIBUTING.md](CONTRIBUTING.md#setup-once).

## Frontend and backend at the same time

The app and the backend are separate programs that share one set of Rust types (`crates/proto`), so each side can run, restart and change on its own:

| You work on | Run | Use |
|---|---|---|
| The app | `cargo dev` | The mock backend plays scripted runs: no keys, no model, no desktop |
| The backend | `cargo run -p agent -- serve --mock` | `HARNESS_URL=http://127.0.0.1:7878 cargo run -p app` in a second terminal to see it, or the tests |
| Both | Both commands, in two terminals | The app reconnects whenever the backend restarts |

`cargo dev` uses a backend that's already running on 127.0.0.1:7878 instead of starting its own. `cargo run -p app` on its own starts the scripted mock inside the app, unless `HARNESS_URL` names a backend.

## What works now

Everything below runs without keys, a model or a desktop: the model and the screen are mocks until the real backends land.

| Folder | What's there | Works now | Stub (not built yet) |
|---|---|---|---|
| `crates/proto` | The contract: types shared by the app, server, agent loop and devices | Run API types and events, `Action` / `Observation` / `ActionResult` / `Refusal`, neutral `Message` / `ContentBlock` / `ToolSpec` | Hub <-> device messages |
| `crates/api` | HTTP + WebSocket routes (axum) behind the `Backend` trait | Every route, `MockBackend` (scripted runs), contract tests | A backend that runs the real agent loop |
| `crates/client` | A typed client for the API; the app talks to the backend only through it | All routes, the live event stream, the in-process mock (`mock` feature), tests against the mock | |
| `crates/app` | The desktop app (GPUI, with GPUI Kit components), one file per view | Start a run, run list, a live run with Stop, connection bar; built-in mock; `format.rs` and `net.rs` tested | Screenshots in the run view, provider/model picker |
| `crates/agent` | The agent loop (lib) and the `harness` binary | The observe -> act -> verify loop (one action per turn, `done` only after a fresh observation, turn and cost budgets, stop at any time: Ctrl-C in `harness run`), each step's screenshot handed to the caller, `harness run --mock`, `runs/<id>.jsonl` logs, `harness serve --mock`, `harness providers` | `serve` without `--mock`, `harness.toml` config, keys, `harness login` |
| `crates/llm` | Model providers behind one `Provider` trait | The trait, `Caps`, `ProviderConfig`, the adapter registry, `MockProvider` | Every real adapter: `anthropic`, `openai`, `openai_compat`, `gemini`, `codex`, `claude_code`, `antigravity` (they error "isn't built yet") |
| `crates/hands` | Screens and input behind one `Hands` trait | `fake::FakeSettings` (enforces tokens and bounds), `scale` (image <-> screen coordinates), tested | Window mode on Windows, macOS and Linux (`src/os/`), the Docker private desktop, the accessibility outline |
| `crates/device` | The agent that runs on a member's PC | | All of it: it prints "not built yet" |
| `crates/xtask` | `cargo dev` | Builds, starts `harness serve --mock`, opens the app | |

## Next tasks

One per crate, so people can work in parallel. Claim one by opening an issue (or commenting on it) and branching `<your-name>/<task>`; see [CONTRIBUTING.md](CONTRIBUTING.md).

- **hands, window mode, one owner per OS:** capture one window, read it as an outline and act on it, in `crates/hands/src/os/{windows,macos,linux}.rs` (plan, section 10.2: same fixture tests on all three). Keep the `WindowMode::new` + `Hands` shape.
- **hands, private desktop:** a `Hands` backend for a Docker Xvfb desktop (screenshot, mouse and keyboard over the whole screen).
- **llm, `anthropic` adapter:** Messages API with the native computer tool, mapped onto `proto::Action`. Test against recorded replies, never real keys in CI.
- **llm, `openai_compat` adapter:** OpenRouter, Ollama, LM Studio and friends by base URL, with the generic `computer` tool. Then `openai`, `gemini` and the plan CLIs (`codex` first).
- **agent, a real backend for the API:** an `api::Backend` that runs `agent::run`, so `harness serve` without `--mock` works (start with `MockProvider` + `FakeSettings`, then real providers). Plus `harness.toml` config and keys from the OS keychain or env.
- **agent, memory and loop checks:** the one-line trail for older observations, `note`, and the `no_visible_progress` refusal (plan, section 10.4), tested with `MockProvider`.
- **device:** the binary that dials the hub over a WebSocket, runs one action at a time through `hands`, and enforces tokens, budgets and the person's input winning (plan, section 10.6).
- **app:** show each step's screenshot in the run view, and a provider/model picker fed by `GET /api/providers`.
- **proto:** the hub <-> device messages, agreed in a `contract` issue first.

Read [CONTRIBUTING.md](CONTRIBUTING.md) before your first PR.
