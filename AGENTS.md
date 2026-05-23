# Project Agent Rules

## Basic Rules

- Always respond in Simplified Chinese for this repository.
- Do not run `git commit`, `git push`, create tags, or publish releases unless the user explicitly asks for that operation.
- Keep this app Windows-first. The native keyboard hook and release workflow are scoped to Windows.
- Do not commit local secrets, signing keys, generated installers, updater signatures, or build output.

## Auto Update Contract

- Auto update uses Tauri's signed updater through GitHub Releases.
- The updater endpoint is:

```text
https://github.com/coderDJing/keyboard-lock-osd/releases/latest/download/latest.json
```

- The updater public key lives in `src-tauri/tauri.conf.json`.
- The matching private key is local-only at:

```text
C:\Users\coder\.tauri\keyboard-lock-osd.key
```

- Never paste the private key into source files, README examples, logs, issues, or release notes.
- GitHub repository secrets required for release signing:

```text
TAURI_SIGNING_PRIVATE_KEY
TAURI_SIGNING_PRIVATE_KEY_PASSWORD
```

- `TAURI_SIGNING_PRIVATE_KEY` must contain the private key file content.
- The current key has no password, so `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` may be empty or omitted unless the key is replaced.

## Version Rules

- Before any release, keep these versions identical:

```text
package.json
src-tauri/Cargo.toml
src-tauri/tauri.conf.json
```

- The git tag must match that version with a `v` prefix.
- Example: app version `0.1.1` uses tag `v0.1.1`.
- Do not publish prerelease or draft releases for normal updater delivery. The app reads GitHub's latest published release.

## Release Workflow

- Release workflow file:

```text
.github/workflows/release.yml
```

- The workflow runs on:

```text
push tags matching v*
workflow_dispatch
```

- The workflow builds on `windows-latest` only.
- It uses `tauri-apps/tauri-action@v0.6.2`.
- It must keep:

```text
releaseDraft: false
prerelease: false
updaterJsonPreferNsis: true
args: --ci
```

- `args: --ci` is required so Tauri signing never waits for interactive input in CI.
- The workflow must upload the Windows installer, updater signature, and `latest.json`.

## Release Steps

1. Update the version in `package.json`, `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json`.
2. Run local validation:

```powershell
pnpm run build
cargo check --manifest-path src-tauri/Cargo.toml
```

3. For updater signing validation, run a local debug NSIS bundle with the private key content in the environment:

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY = Get-Content -Raw "C:/Users/coder/.tauri/keyboard-lock-osd.key"
pnpm tauri build --debug --bundles nsis --ci
```

4. Confirm the bundle directory contains both files:

```text
src-tauri/target/debug/bundle/nsis/*.exe
src-tauri/target/debug/bundle/nsis/*.exe.sig
```

5. Only after explicit user approval, create and push the version tag:

```powershell
git tag v0.1.1
git push origin v0.1.1
```

6. Watch the GitHub Actions `Release` workflow until it completes successfully.
7. Confirm the GitHub Release is published and contains `latest.json`.

## Verification Notes

- `pnpm run build` validates TypeScript and frontend build.
- `cargo check --manifest-path src-tauri/Cargo.toml` validates Rust and Tauri config compilation.
- Local signed debug bundling validates updater artifact signing.
- A release build checks for updates only outside debug builds.
