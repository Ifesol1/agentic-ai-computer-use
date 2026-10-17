# Rules for coding agents

Read CONTRIBUTING.md. The short version:

- Before every commit: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`. All three must pass.
- One task = one branch = one worktree. Stay inside the crate your task names; ask before changing another crate's public types.
- `crates/proto` is the contract with the frontend. Change it only when the task says so. When you do: update `MockBackend` (crates/api/src/mock.rs) so the mock shows the change, run `cargo test` (it regenerates `web/src/gen`), commit both, and bump `CONTRACT_VERSION` for a breaking change.
- Never edit `web/src/gen/` by hand.
- No `u64`/`i64` in `proto` types: ts-rs turns them into `bigint`. Use `u32` or `f64`.
- Routes in `crates/api` only call the `Backend` trait. New behaviour goes into the backends, never into a route.
- Never commit keys, tokens or `.env` files, and never print them in logs, issues or PRs.
