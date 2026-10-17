# web: the frontend

The frontend team owns this folder and picks the framework. What it gets from the backend:

- **Types:** `src/gen/*.ts`, generated from `crates/proto` whenever anyone runs `cargo test`. Import them; never edit them.
- **A fake backend:** `cargo run -p agent -- serve --mock` serves the real API with scripted runs on http://127.0.0.1:7878.

## The API (contract v0)

| Method | Path | Body → answer |
|---|---|---|
| GET | `/api/health` | → `Health` |
| GET | `/api/providers` | → `ProviderInfo[]` |
| GET | `/api/runs` | → `RunInfo[]`, newest first |
| POST | `/api/runs` | `StartRun` → 201 `RunInfo` |
| GET | `/api/runs/{id}` | → `RunInfo` |
| POST | `/api/runs/{id}/stop` | → `RunInfo` |
| GET (WebSocket) | `/api/runs/{id}/events` | → `RunEvent` per text frame: every event so far, then live ones; closes after `finished` |
| GET | `/api/runs/{id}/steps/{n}/screenshot` | → an image (SVG in the mock) |

Errors are 4xx with an `ApiError` body.

## Talking to the API in development

Point your dev server's proxy at the mock, so the browser sees one origin. With Vite:

```ts
// vite.config.ts
export default defineConfig({
  server: { proxy: { "/api": { target: "http://127.0.0.1:7878", ws: true } } },
});
```

No proxy? Start the mock with `--allow-origin http://localhost:5173`.

## Starting an app here

Scaffold your framework into this folder and keep `src/gen`. For example, from the repo root: `npm create vite@latest web -- --template react-ts`, and choose to ignore existing files.
