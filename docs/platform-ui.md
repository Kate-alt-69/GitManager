# GitManager platform UI boundary

Status: documented integration seam; wl_ui is not wired into this repository.

## Latest wl_ui state checked

The latest visible WinLook commit reviewed for this boundary is
`e803ec4e919a00c881dd5b3429a79b0293e74457` (2026-11-13), which split the
facade into explicit platform modules and isolated the Windows 10 / Windows 11
toolchain paths. The current `rust/library/wl_ui` package is `0.1.0-alpha.1`.

The facade exposes these feature-gated renderer modules:

- `wl_ui::windows10` — lightweight native Win32 / User32.
- `wl_ui::windows11` — native WinUI 3 / Windows Reactor.
- `wl_ui::tauri2` — optional adapter for an existing Tauri 2 app.
- `wl_ui::windows7`, `wl_ui::windows8`, and `wl_ui::windows81` — experimental
  classic Win32 route.

The platform-neutral model exports `UiApplication`, `UiPage`,
`UiWindowConfig`, and `UiError`. The current README/source describes the
application shell and page navigation model. It does not yet expose a Styling
API, theme tokens, control styling contract, or a production layout/control
toolkit. The missing Styling API is an explicit blocker for GitManager's native
UI migration.

## Intended target selection

- **Windows 10:** use the explicit `windows10` feature and native Win32/User32.
- **Windows 11:** use the explicit `windows11` feature and native WinUI 3 /
  Windows Reactor.
- **Linux and macOS:** continue using GitManager's existing Tauri 2 host.
  Windows-only wl_ui modules must not be resolved or compiled for these targets.

Renderer selection is explicit Cargo feature selection in the consuming
application. GitManager must not assume that wl_ui automatically chooses a
backend at runtime.

## Integration boundary

Keep GitManager's repository scanner, Git operations, account providers,
repository-local preferences, update service, and cleaner commands in the Rust
application backend. A host adapter should consume those operations through a
narrow interface; repository logic must not move into renderer-specific pages.

Do not integrate the current `wl_ui::tauri2::attach` yet. Its implementation
creates a new `wl-ui` webview window rather than adapting GitManager's existing
main window. Using it as-is risks a duplicate window and does not provide the
missing Styling API. Do not call the standalone WinLook Manager model or
renderer entry points: those create WinLook's Manager shell, not GitManager's
repository dashboard.

The integration should wait for both:

1. A documented Styling API covering theme/style selection, color and
   typography tokens, spacing, layout, and control states.
2. A host-adapter contract that attaches to GitManager's existing application
   window without starting a second window or duplicate command runtime.

Until both are available, Tauri remains the buildable host on all CI targets.
This is deliberate staging, not a claim that GitManager already uses wl_ui.

## Required checks when the API arrives

- Windows 10: compile and runtime-test the `windows10` host.
- Windows 11: compile and runtime-test the `windows11` host.
- Linux and macOS: build without Windows-only wl_ui or Windows Reactor
  dependencies.
- Run the same scanner, preferences, account, cleaner, and background-service
  tests against each supported host.
- Verify one GitManager application window and one command runtime per process.
- Verify the Styling API preserves GitManager's repository cards, branch
  navigation, CI status, and commit hover details rather than replacing them
  with WinLook's placeholder Manager navigation.
