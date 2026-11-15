# Where the project stands, and the next step

Date: 2026-11-15. Written from the repo, its open PRs, issues, branches and CI runs.

## 1. The shape of the repo

`main` has only the initial commit (README + .gitignore). All real code is on
`ife/gpui-base` (PR #1, "Rust backbone", open since 2026-10-29, unmerged). It is a
Rust workspace, one crate per responsibility:

| Crate | Role | State on `ife/gpui-base` |
|---|---|---|
| `proto` | Shared contract (run API, `Action`/`Observation`, messages) | Done, `CONTRACT_VERSION = 0`; hub <-> device messages missing |
| `api` | axum HTTP + WebSocket routes behind a `Backend` trait | Done, with `MockBackend` and contract tests |
| `client` | Typed Rust client for the API | Done |
| `app` | GPUI desktop app | Runs against the mock; no screenshots, no provider picker |
| `agent` | The observe -> act -> verify loop, `harness` binary | Loop works with mocks; `serve` without `--mock` is a stub |
| `llm` | `Provider` trait + adapter registry | Only `MockProvider`; every real adapter is a stub |
| `hands` | `Hands` trait (observe/act) | Only the fake Settings window; no OS window mode, no private desktop |
| `device` | Agent that runs on a member's PC and dials the hub | Prints "not built yet" |

The README names a "plan" document (sections 10.2, 10.4, 10.6) that is not in the repo.
Anyone picking up `device` or `proto` needs it. Ask the author of PR #1 to add it under `docs/`.

## 2. Work in flight (stacked on `ife/gpui-base`)

| README task | Owner | Where | Status |
|---|---|---|---|
| app: screenshots + provider picker | Marco | PR #4 (`marco/app-screenshots`) | CI green. Also fixes the Windows client test |
| agent: memory and loop checks | Marco | PR #5 (`marco/agent-memory`) | CI red on Windows only, same pre-existing bug PR #4 fixes |
| agent: real backend for the API | Lokesh | issue #2, branches `lokesh/agent-stop`, `lokesh/api-backend`, `lokesh/config` | Pushed, no PR yet |
| llm: `anthropic` adapter | Lokesh | issue #3, `lokesh/anthropic-adapter` | Pushed, no PR yet |
| hands: private desktop (X11) | Lokesh | `lokesh/x11-desktop` | Pushed, not claimed in an issue |

## 3. Unclaimed README tasks

- **hands, window mode per OS** (Windows, macOS, Linux files)
- **llm, `openai_compat` adapter** (OpenRouter, Ollama, LM Studio), then `openai`, `gemini`, `codex`
- **device**: the binary that dials the hub over a WebSocket and runs actions through `hands`
- **proto: hub <-> device messages**, agreed in a `contract` issue first

## 4. What blocks everyone right now

1. **PR #1 is not merged.** Five branches and two PRs are based on it. Until it lands,
   `main` is empty and every PR targets a feature branch, so nothing can be reviewed
   against `main` and the README "Quick start" is false for anyone who clones.
2. **CI is red on Windows** for every branch except PR #4. The failing test is
   `follows_a_run_to_the_end` in `crates/client/tests/against_mock.rs`
   (os error 10053: the client keeps reading after the server closes the socket).
   PR #4's commit "client: ... end the event stream after finished" fixes it.
3. **The plan document** referenced by the README is not in the repo.

## 5. Recommended next step, in order

1. **Merge PR #1 into `main`** (owner's call: Ifesol1). Its only CI failure is the
   Windows test above, which is a test-harness bug, not a product bug.
2. **Merge PR #4** next, since it carries the client fix that makes Windows CI green
   for everyone else. Then PR #5. Then ask Lokesh to open PRs for issues #2 and #3.
3. **Re-target** the remaining branches to `main` once PR #1 is in.
4. **Claim a task.** The two unclaimed tasks that unlock remote and mobile use are
   `proto: hub <-> device messages` and `device`. They are also the prerequisite for
   the phone work in `docs/mobile-integration-research.md`: a phone can already talk
   to `harness serve` on the LAN today through the existing API, but reaching a PC
   from outside the LAN needs the hub, and the hub needs this contract.

   Suggested first move: open a `contract` issue proposing the hub <-> device message
   set (register, heartbeat, `observe`, `act`, `result`, `refused`, `person_active`,
   `budget`), keyed on the existing `Target::Window { device, window }` and the
   `Hands` trait, so the device is just a remote `Hands`.
5. **Add `docs/plan.md`** (or link it) so sections 10.2, 10.4, 10.6 are readable.

## 6. Small things noticed

- README "Next tasks" still lists tasks that are claimed (app, agent memory,
  anthropic, api backend). Add the owner or strike them when PR #1 is updated.
- `lokesh/x11-desktop` is the "hands, private desktop" task but has no issue. Ask
  Lokesh to claim it so no one duplicates it.
- CI uses `actions/checkout@v4`; GitHub warns Node 20 is deprecated. Bump to v5 when
  convenient.
