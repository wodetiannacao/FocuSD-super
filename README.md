# FocuSD Island

A Windows-first Tauri + React floating island shell.

## Current MVP

- Transparent, borderless, always-on-top island window.
- Primary-display top-center positioning.
- Collapsed capsule state and expanded panel state.
- React content slot for future modules.
- System tray menu with show, hide, and quit actions.

## Development

Prerequisites:

- Node.js and pnpm
- Rust/Cargo
- Microsoft Visual Studio Build Tools with the C++ workload
- Microsoft Edge WebView2 Runtime

Run the app in development mode:

```powershell
pnpm tauri dev
```

Build the release executable:

```powershell
pnpm tauri build --no-bundle
```

The release executable is written to:

```text
src-tauri/target/release/focusd-island.exe
```
