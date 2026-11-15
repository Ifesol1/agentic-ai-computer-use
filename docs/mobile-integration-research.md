# Mobile integration: a phone that drives the PC agent

Date: 2026-11-15. iPhone first, Android second. Written for this repo: the PC side is
the Rust `harness` (crates/api + crates/agent), not a new Python server.

## 1. The key fact: the phone is just another client of the existing API

The app already talks to the backend only through the HTTP + WebSocket API in
`crates/api` (`POST /api/runs`, `GET /api/runs/{id}/events`, `POST /api/runs/{id}/stop`,
`GET /api/runs/{id}/steps/{n}/screenshot`, `GET /api/providers`). A phone client needs
nothing new from the backend to start a run and watch it live. What the phone cannot do
today:

1. **Reach the PC from outside the LAN.** That is the `hub` + `device` design from the
   README (unclaimed tasks "device" and "proto: hub <-> device messages").
2. **Approve or deny a step.** There is no approval gate in the run API yet. A
   `RunEvent::ApprovalRequired { step, action }` plus `POST /api/runs/{id}/approve`
   would be a non-breaking contract addition (new event type, new route).
3. **Authenticate.** The API has no auth. Fine on 127.0.0.1, not fine on a LAN or hub.

Anthropic's computer use is now the stable `computer_toolset_20260801` toolset on the
Claude API; its docs recommend a VM/container, a domain allowlist, and human confirmation
for consequential actions (https://platform.claude.com/docs/en/agents-and-tools/tool-use/computer-use-tool).

## 2. Architectures

**(a) Thin client / remote control.** Phone sends the prompt to `POST /api/runs` and
follows the events WebSocket. Connectivity:

| Path | Notes |
|---|---|
| LAN only | Simplest; club demos. `harness serve` must bind 0.0.0.0, with a token. |
| Tailscale | Private WireGuard mesh; the phone reaches the PC as if on LAN, nothing public. Also gives real HTTPS certs. (https://ngrok.com/compare/tailscale) |
| Cloudflare Tunnel | Outbound-only connector gives an HTTPS URL; add Cloudflare Access or anyone with the link can hit it. |
| ngrok | One-command public URL for demos; free-tier limits. |
| Club hub (the README design) | `device` dials the hub over a WebSocket, no port forwarding; the phone and the GPUI app both talk to the hub. The long-term answer. |

**(b) Human-in-the-loop viewer.** Every `Step` already carries a `screenshot_url`, so a
phone UI gets the after-step image for free. For full-motion video, pair with Sunshine
(host) + Moonlight (iOS/Android client): free, self-hosted, about 20 ms on LAN
(https://apps.apple.com/app/id1000551566).

**(c) Phone as input.** Camera photo attached to the prompt (needs a `StartRun` image
field, non-breaking optional addition), voice via Siri dictation, location via Shortcuts.
Anthropic's own Remote Control does exactly this for Claude Code
(https://code.claude.com/docs/en/remote-control).

## 3. iPhone mechanisms

| Mechanism | What it gives | Catches |
|---|---|---|
| **Shortcuts, "Get Contents of URL"** | Zero-code MVP: "Hey Siri, tell the agent ..." dictates text and POSTs JSON to `/api/runs`. | Rejects self-signed certs and some plain-HTTP setups; nested JSON bugs. Use HTTP on LAN first, or Tailscale HTTPS. (https://support.apple.com/guide/shortcuts/apd58d46713f/ios) |
| **App Intents (native Siri)** | Proper Siri verbs. | Needs a Swift app; intents get about 30 s, so fire-and-forget and report by push. iOS 27 adds `LongRunningIntent`. |
| **SwiftUI + `URLSessionWebSocketTask`** | Native live view of the events socket. | Socket dies seconds after backgrounding; reconnect on foreground (the API replays history, so this is cheap). |
| **PWA** | Chat + screenshot viewer, installable, works on Android too. | Web Push only when added to the home screen (iOS 16.4+), no background sync, 7-day storage eviction. (https://firt.dev/notes/pwa-ios/) |
| **Push for approvals** | APNs needs the paid account. **ntfy** (free app, topic-based, action buttons) or Pushover (one-time, about 5 USD) work with no account. Several Claude Code approve/deny hooks already use ntfy (https://github.com/yuuichieguchi/claude-remote-approver). | Self-hosted ntfy must relay through ntfy.sh for instant iOS delivery. |
| **Signing** | Free Apple ID: sideload to 3 devices, 7-day expiry, needs a Mac. Developer Program (99 USD/yr): TestFlight, 90-day builds. | No free path from Windows or Linux. (https://expo.dev/blog/apple-developer-program-explained-free-vs-paid) |
| **Cross-platform** | **Expo / React Native**: EAS cloud build signs iOS from any OS, free push service. **Flutter**: also fine, but iOS signing still needs a Mac or paid CI. | |

## 4. Prior art worth copying

- **Anthropic Remote Control** (`claude remote-control`, QR pairing from the Claude iOS/Android app, photos attached from the phone, relay via Anthropic). Claude Code only, subscription only. UX reference.
- **Happy Coder** (MIT, Expo app + CLI wrapper, end-to-end encrypted relay, push, Claude Code and Codex): the best open-source template for a student team (https://github.com/slopus/happy).
- **Omnara** (open-source core, web + iOS + Watch, approvals, diffs, voice) (https://github.com/omnara-ai/omnara).
- **Open Interpreter 01 App**: phone as voice client to a local server (https://github.com/openinterpreter/01).
- **OpenAI Operator, Google Project Mariner**: folded into ChatGPT agent mode and Gemini; cloud-hosted, not controllable from your own PC. UX references only.
- **SSH approach**: Termius or Blink + tmux over Tailscale gives a terminal to `harness` with zero app code.

## 5. Security rules for this project

1. Never port-forward `harness serve`. LAN, Tailscale, Cloudflare Access, or the club hub.
2. Per-device bearer token, shown as a QR at pairing (like Remote Control and Happy). Add
   an `Authorization` check as a tower layer around `api::router`, not in the routes.
3. TLS from Tailscale or Cloudflare; Shortcuts will not accept self-signed certs anyway.
4. Keep the existing safety shape: single-use observation tokens, budgets, the person's
   input winning (`Refusal::AppInUse`). The phone adds one more gate: approval before
   consequential actions, default deny on timeout, every action logged with its screenshot.
5. Screenshots can carry prompt injection; the approval gate is the defence.

## 6. Android (shorter)

Same architecture. Advantages: free sideloading (APK or Play internal testing);
foreground services keep the events WebSocket alive; Firebase Cloud Messaging or ntfy via
UnifiedPush for approvals, free; Tasker and Termux can fire HTTP requests or even host a
client; the Accessibility Service lets the phone itself become an agent target later
(PokeClaw, DroidPilot). Strategy: build once in Expo, ship the Android APK at once, keep
iOS on 7-day sideloads until the club buys the developer account.

## 7. Phased plan for the club

**Phase 0, backend (1 week, Rust, this repo).**
- `harness serve --bind 0.0.0.0:7878 --token <t>` with a bearer-token layer.
- Non-breaking contract additions: `RunEvent::ApprovalRequired`, `POST /api/runs/{id}/approve`,
  optional `StartRun.images`. Update `MockBackend` so the mock shows them (CONTRIBUTING.md).

**Phase 1, MVP with no app code (weeks 2-3, 0 USD).**
- iPhone Shortcut: dictate, POST to `/api/runs` over LAN or Tailscale.
- Backend posts progress and approval prompts to an ntfy topic with Approve/Deny action URLs.

**Phase 2, PWA (weeks 4-6, 0 USD).**
- A small web page served by `harness` (or a new `crates/web`): prompt box, run list, live
  steps with screenshots, approve buttons. Installable on iOS and Android.

**Phase 3, native app (months 2-3).**
- Expo app: QR pairing, WebSocket live view, camera and voice input, push via
  expo-notifications. Android free; iOS via TestFlight once the club has an account.
- Off-LAN access through the club hub once `device` and the hub contract land.

## 8. Comparison

| Approach | Effort | Cost | iOS friction | Capability |
|---|---|---|---|---|
| Shortcuts + ntfy | Hours | 0 | Low (HTTPS quirks) | Send tasks, get alerts, basic approve |
| SSH/tmux + Tailscale | Hours | 0 | Low | Full terminal, no screenshots |
| PWA | 1-2 weeks | 0 | Medium (push flaky, no background) | Chat, screenshots, approvals |
| Expo native | 3-6 weeks | 0 Android, 99 USD/yr iOS | Medium-high (signing) | Everything, reliable push, camera, voice |
| SwiftUI + App Intents | 4-8 weeks | 99 USD/yr + Mac | High | Best Siri; iOS only |
| Anthropic Remote Control | Zero | Pro/Max | None | Claude Code only, not this agent |
| Sunshine/Moonlight | Hours | 0 | Low | Live video, no agent UI |
