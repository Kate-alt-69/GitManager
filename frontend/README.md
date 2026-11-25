# GitManager frontend reset

The previous application UI has been intentionally removed. The root route
(`/`) is the only application route and currently renders no view while the
replacement UI is designed and wired.

## Preserved backend-facing boundary

These files are deliberately retained without modification:

- `lib/tauri.ts` — Tauri command, event, and native-dialog bridge.
- `lib/types.ts` — shared contracts for repositories, accounts, preferences,
  notifications, task state, and related backend payloads.
- `lib/use-gitmanager.ts` — existing React data/service hook that calls the
  current Tauri commands and subscribes to backend events.
- Rust/Tauri code under `../src-tauri` and core/service crates under
  `../crates` remain the source of truth for application behavior.

## Runtime contract

`lib/runtime-bridge.tsx` mounts the preserved `useGitManager()` hook and
continues reporting the WebView heartbeat every three seconds. The heartbeat
is required by the native crash supervisor. Keep this bridge mounted until
the replacement UI explicitly takes over those responsibilities.

## Build boundary

The Next.js/Tauri build and packaging pipeline remains unchanged. The
replacement UI can be introduced under the existing App Router without
changing the backend command contract.
