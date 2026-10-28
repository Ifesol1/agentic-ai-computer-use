# Rules for coding agents

Read CONTRIBUTING.md. The short version:

- Before every commit: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`. All three must pass.
- One task = one branch = one worktree. Stay inside the crate your task names; ask before changing another crate's public types.
- `crates/proto` is the contract between the app and the backend. Change it only when the task says so. When you do: update `MockBackend` (crates/api/src/mock.rs) so the mock shows the change, fix both sides, and bump `CONTRACT_VERSION` for a breaking change.
- Routes in `crates/api` only call the `Backend` trait. New behaviour goes into the backends, never into a route.
- The app (`crates/app`) talks to the backend only through `crates/client`. Network calls go through `Net::call` / `Net::spawn` (crates/app/src/net.rs), never directly on GPUI's executor. Text the UI shows goes in `views/format.rs`, with a test.
- Never commit keys, tokens or `.env` files, and never print them in logs, issues or PRs.
