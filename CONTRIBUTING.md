# Contributing

## Setup, once

1. Install Rust with rustup: https://rustup.rs. `rust-toolchain.toml` pins the version, and rustup downloads it on your first build.
2. Recommended, saves minutes per worktree: `cargo install sccache --locked`, then add to `~/.cargo/config.toml`:

   ```toml
   [build]
   rustc-wrapper = "sccache"
   ```

3. Clone and build:

   ```bash
   git clone https://github.com/Ifesol1/agentic-ai-computer-use.git
   cd agentic-ai-computer-use
   cargo test
   ```

## Run the fake backend

```bash
cargo run -p agent -- serve --mock          # http://127.0.0.1:7878
cargo run -p agent -- serve --mock --mock-step-ms 200   # faster runs
```

It plays a scripted "turn on dark mode" run with screenshots. A prompt containing "fail" gives a failed run, and `"max_steps": 2` a stopped one.

## Branches, worktrees, PRs

- Branch name: `<your-name>/<task>`, for example `ife/click-tool`.
- One task = one branch = one worktree, so agents and people never share files:

  ```bash
  git fetch origin
  git worktree add ../wt/ife-click-tool -b ife/click-tool origin/main
  cd ../wt/ife-click-tool
  # work, commit, push, open a PR; then from the main clone:
  git worktree remove ../wt/ife-click-tool
  ```

- Keep a PR to one crate where you can. CI must pass and one person must approve.
- `Cargo.lock` is committed. When it conflicts: take main's copy, run `cargo build`, commit.

## Changing the contract (`crates/proto`)

The types in `crates/proto` are what the frontend codes against, and `web/src/gen` is generated from them.

1. Open an issue labelled `contract` that says what changes and why, and tag someone from the frontend.
2. In the PR: change the type, make `MockBackend` show the change, run `cargo test` (it rewrites `web/src/gen`), and commit both. CI fails if `web/src/gen` is out of date.
3. Non-breaking: a new optional field, a new event type, a new route. Breaking: renaming or removing a field, changing a field's type, a new required field. For a breaking change, bump `CONTRACT_VERSION` and put `[breaking]` in the PR title.
4. A contract PR needs one backend and one frontend approval.
