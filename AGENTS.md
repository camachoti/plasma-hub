# AGENTS.md

## Purpose

`plasma-hub` is a Rust application for desktop and Android with a Dioxus 0.7 UI and a domain workspace independent of the renderer. The React/Vite/Tauri implementation was retired in the Phase 8 cutover; its source is kept read-only in `reference/react-frontend/` for consultation only.

## Project Layout

The repository is a single Cargo workspace.

- `crates/plasma-core`: shared errors, app paths, event sink (`AppEnv`).
- `crates/plasma-platform`: filesystem and OS helpers.
- `crates/plasma-store`: SQLite message cache, download history, topic state, settings, and schema migrations.
- `crates/plasma-download`: cancellation, streamed HTTPS downloads, file sinks.
- `crates/plasma-extract`: unified platform analysis/download dispatch (YouTube, TikTok, Instagram, Reddit, Twitter, direct web).
- `crates/plasma-media`: media cache helpers and video thumbnails.
- `crates/plasma-telegram`: TDLib integration and the local media HTTP server.
- `crates/plasma-twitter`, `plasma-tiktok`, `plasma-youtube`: native extractors/downloaders.
- `crates/plasma-duplicates`: duplicate media search.
- `crates/plasma-instagram`, `plasma-reddit`: migration placeholders.
- `crates/plasma-ui`: Dioxus views, theme, virtualized timeline, shared controls and CSS; renderer-independent.
- `apps/plasma`: desktop/Android composition root and Dioxus renderer (package `plasma`, member of the root workspace).
- `reference/react-frontend`: old React/TypeScript/Vite frontend kept as reference. It is not compiled, tested or packaged.
- `docs/rust-rewrite-plan.md`: migration plan and history.
- `scripts/`: desktop packaging, Android build/install and toolchain checks.

Domain crates must not depend on `dioxus`, `tao` or `wry`; they receive
`plasma_core::AppEnv` and return domain types. The app layer only handles
platform startup and maps UI actions. See `docs/rust-rewrite-plan.md`.

## Core Commands

- `cargo run -p plasma`: run the desktop app.
- `cargo build -p plasma --release`: optimized desktop build.
- `cargo fmt --all`: format the workspace.
- `cargo clippy --workspace --all-targets -- -D warnings`: lint everything.
- `cargo test --workspace`: run the Rust tests.
- `cargo check -p plasma`: fast check of the app shell.
- `scripts/bundle-dioxus-desktop.sh`: build the `.deb` and the portable directory in `dist/` (requires Dioxus CLI 0.7.10).
- `scripts/check-android-toolchain.sh`: validate the Android environment.
- `scripts/build-dioxus-android.sh [debug|release]`: build the arm64 APK.
- `scripts/install-android-apk.sh [apk]`: install the latest (or given) APK with adb.

## Environment Expectations

- Desktop builds need GTK 3, WebKitGTK 4.1, libsoup 3, OpenSSL, libxdo and the C++ runtime required by TDLib. Packaging also needs `dx` 0.7.10.
- Android builds require a full JDK, Android SDK and Android NDK.
- Use `scripts/build-dioxus-android.sh` for mobile validation instead of plain Cargo target checks because it injects the linker/toolchain environment and packages `libtdjson` into the APK.
- `PLASMA_HOME` isolates app data during development or smoke tests.
- `apps/plasma/build.rs` embeds `PLASMA_TELEGRAM_API_ID/HASH` or the legacy `VITE_API_ID/HASH` from the environment or the root `.env`; runtime variables take precedence.
- Distribution is manual for now: `.deb` plus portable directory. There is no auto-updater or signed release pipeline yet.

## Working Rules

- Keep changes aligned with the Rust/Dioxus patterns in the repo. UI work belongs in `crates/plasma-ui`; startup and platform wiring belong in `apps/plasma`.
- Put native behaviour in `crates/*`; keep the app shell thin.
- Run `cargo fmt --all` before committing Rust changes.
- Preserve desktop behaviour while treating Android as an active compatibility target.
- Prefer small wrappers around platform-specific behaviour instead of branching feature logic across the app shell.
- Do not revert unrelated user changes already present in the worktree.
- Treat `reference/react-frontend` as historical material: do not wire it into the build or copy new features from it without porting them to Rust.

## Validation

Use the smallest relevant validation set for the change:

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo check -p plasma`
- `scripts/check-android-toolchain.sh`

For packaging changes, also run `scripts/bundle-dioxus-desktop.sh` and smoke test `dist/plasma-linux-x86_64/plasma` with `PLASMA_HOME` pointing to a development data directory.

For Android-specific work, validate with:

- `scripts/check-android-toolchain.sh`
- `scripts/build-dioxus-android.sh debug`

## Current Product Constraints

- TDLib native login/downloads work on desktop and are expected to work on Android.
- Twitter/X native downloads work on desktop and are expected to work on Android.
- YouTube direct and signed stream downloads are supported on Android, but adaptive video-only formats are hidden there unless audio is already included.
- Opening or revealing files is desktop-first; Android may fall back to app/system paths where available.
- Android uses `network_security_config` with cleartext only for `127.0.0.1` (playback and thumbnails through the loopback server).
- The SQLite schema, `plasma-hub.sqlite3`, cookies, media cache and TDLib data remain compatible with the pre-cutover app.
