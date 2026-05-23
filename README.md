# Keyboard Lock OSD

Keyboard Lock OSD is a lightweight Windows app for Caps Lock, Num Lock, and Scroll Lock state changes.

## Current Scope

- Event-driven Windows keyboard hook for immediate lock-key feedback.
- Minimal bottom-center OSD using short English labels: `CAP`, `NUM`, `SCRL`.
- Bilingual settings window structure for English and Chinese.
- Optional fullscreen suppression for the OSD, enabled by default.
- Silent tray-first startup; settings opens from the tray menu or tray icon.
- Signed auto-updates through GitHub Releases.
- Tauri 2 + Rust + React + Vite.

## Development

```powershell
pnpm install
pnpm tauri dev
```

## Release And Auto Update

The app uses Tauri's signed updater. Release builds check GitHub Releases on startup and install a newer signed Windows installer when one is available.

Updater endpoint:

```text
https://github.com/coderDJing/keyboard-lock-osd/releases/latest/download/latest.json
```

The updater public key is stored in `src-tauri/tauri.conf.json`. The matching private key was generated locally at:

```text
C:\Users\coder\.tauri\keyboard-lock-osd.key
```

Configure these GitHub repository secrets before publishing a release:

```text
TAURI_SIGNING_PRIVATE_KEY
TAURI_SIGNING_PRIVATE_KEY_PASSWORD
```

`TAURI_SIGNING_PRIVATE_KEY` must contain the private key file content. The current key was generated without a password, so `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` can be left empty or omitted unless a password-protected key replaces it.

To publish an update, bump the version in `package.json`, `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json`, then push a version tag:

```powershell
git tag v0.1.1
git push origin v0.1.1
```

The `Release` GitHub Actions workflow builds the Windows Tauri installer, uploads signed updater artifacts, and publishes `latest.json` to the GitHub Release. Keep the release published, not draft, because the app reads from GitHub's `latest` release endpoint.

## Notes

The first implementation targets Windows. The app reads the initial lock-key state on startup, then reacts to global keyboard events instead of polling in a timer.
