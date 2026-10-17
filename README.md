# agentic-ai-computer-use

Agentic AI Club - PC sub team: computer-use agent project

The club's own agent harness: one agent loop and one set of tools, with the model as a setting, that can see and drive a computer.

## Quick start

```bash
# Rust from https://rustup.rs; the pinned version installs itself on first build
git clone https://github.com/Ifesol1/agentic-ai-computer-use.git
cd agentic-ai-computer-use
cargo test                                   # builds everything, regenerates web/src/gen
cargo run -p agent -- serve --mock           # fake backend on http://127.0.0.1:7878
curl http://127.0.0.1:7878/api/runs
```

| Folder | What's there |
|---|---|
| `crates/proto` | The contract: types shared by the frontend, server, agent loop and devices |
| `crates/api` | HTTP + WebSocket routes (axum) and the mock backend |
| `crates/agent` | The `harness` binary |
| `crates/llm` | Model providers behind one trait |
| `crates/hands` | Screens and input behind one trait |
| `crates/device` | The agent that runs on a member's PC (not built yet) |
| `web` | The frontend; `web/src/gen` is generated from `crates/proto` |

Read [CONTRIBUTING.md](CONTRIBUTING.md) before your first PR.
