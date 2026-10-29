# Contributing

## Setup, once

1. Install Rust with rustup: https://rustup.rs. `rust-toolchain.toml` pins the version, and rustup downloads it on your first build.
2. Your OS:
   - **Windows:** accept the Visual Studio C++ build tools when rustup asks. If a build fails with "An Application Control policy has blocked this file (os error 4551)", Smart App Control is on and blocks every program cargo builds. Rust development needs it off: Windows Security > App & browser control > Smart App Control.
   - **macOS:** install the Xcode command line tools: `xcode-select --install`.
   - **Linux (Debian/Ubuntu):** `sudo apt install build-essential pkg-config libxcb1-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libfontconfig-dev libfreetype-dev`. On other distros, install the same libraries.
3. Recommended, saves minutes per worktree: `cargo install sccache --locked`, then add to `~/.cargo/config.toml`:

   ```toml
   [build]
   rustc-wrapper = "sccache"
   ```

4. Clone and build:

   ```bash
   git clone https://github.com/Ifesol1/agentic-ai-computer-use.git
   cd agentic-ai-computer-use
   cargo test
   ```

The first build compiles GPUI and takes a few minutes; later builds only rebuild what changed.

## Running things

```bash
cargo dev                                  # mock backend + the app, together
cargo run -p agent -- serve --mock         # just the backend, on http://127.0.0.1:7878
cargo run -p app                           # just the app, with the mock built in
HARNESS_URL=http://127.0.0.1:7878 cargo run -p app      # the app against a running backend
cargo run -p agent -- serve --mock --mock-step-ms 200   # faster mock runs
cargo run -p agent -- run --mock "turn on dark mode"    # the real agent loop, scripted model, fake window
```

The mock backend plays a scripted "turn on dark mode" run. A prompt containing "fail" gives a failed run, and `"max_steps": 2` a stopped one.

`harness run --mock` is different: it runs the real agent loop (`crates/agent`) with a scripted model (`llm::MockProvider`) against a fake Settings window (`hands::fake::FakeSettings`), prints each step and writes `runs/<id>.jsonl` (git-ignored).

## Before every commit

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

CI runs the same three on Linux, and clippy and the tests on macOS and Windows.

## Branches, worktrees, PRs

- Pick a task from [README.md, "Next tasks"](README.md#next-tasks) and claim it in an issue, so two people don't build the same thing.
- Branch name: `<your-name>/<task>`, for example `ife/click-tool`.
- One task = one branch = one worktree, so agents and people never share files:

  ```bash
  git fetch origin
  git worktree add ../wt/ife-click-tool -b ife/click-tool origin/main
  cd ../wt/ife-click-tool
  # work, commit, push, open a PR; then from the main clone:
  git worktree remove ../wt/ife-click-tool
  ```

- Keep a PR to one crate where you can; in the app, to one view file. CI must pass and one person must approve.
- `Cargo.lock` is committed. When it conflicts: take main's copy, run `cargo build`, commit.

## Changing the contract (`crates/proto`)

The types in `crates/proto` are the API between the app and the backend. Both sides use the same Rust types, so a change that breaks either side fails to compile.

1. Open an issue labelled `contract` that says what changes and why, and tag someone from the other side.
2. In the PR: change the type, make `MockBackend` show the change, and fix both sides until `cargo test` passes.
3. Non-breaking: a new optional field, a new event type, a new route. Breaking: renaming or removing a field, changing a field's type, a new required field. For a breaking change, bump `CONTRACT_VERSION` and put `[breaking]` in the PR title. The app warns when it talks to a backend on another contract version.
4. A contract PR needs one backend and one app approval.
