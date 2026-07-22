# AGENTS.md

## Purpose

This repository is `plasma-hub`, a Tauri 2 application with a React/Vite frontend. The same app shell is intended to work on desktop and Android, with platform-specific behavior isolated behind shared runtime wrappers and Tauri commands.

## Project Layout

- `src/app`: app shell, navigation, login flow, bootstrap hooks.
- `src/features`: product domains such as Telegram, Twitter/X, downloader, and appearance.
- `src/shared/platform`: runtime capability detection, service worker registration, typed Tauri wrappers.
- `src/shared/storage`: storage abstraction used before finalizing desktop/mobile storage backends.
- `src-tauri/src/commands`: commands exposed to the webview.
- `src-tauri/src/services`: native services for TDLib, Twitter/X, YouTube, and Android fallbacks.

## Core Commands

- `npm run dev`: run the Vite frontend only.
- `npm run build`: run TypeScript checks and build the frontend bundle.
- `npm run tauri:dev`: run the desktop Tauri app.
- `npm run tauri:build`: build the desktop Tauri app.
- `npm run tauri:android:init`: initialize the Android project.
- `npm run tauri:android:dev`: run the Android app in development mode.
- `npm run tauri:android:build`: build the Android app for `aarch64`.
- `npm run tauri:android:install`: install the generated Android APK.

## Environment Expectations

- Desktop Tauri commands depend on `scripts/check-tdlib-runtime.sh`.
- Android commands depend on `scripts/check-android-toolchain.sh`.
- Android builds require a full JDK, Android SDK, and Android NDK.
- Use the Tauri Android workflow for mobile validation instead of plain Cargo target checks because the CLI injects required linker/toolchain environment.

## Working Rules

- Keep changes aligned with existing React, TypeScript, and Tauri patterns in the repo.
- Scope frontend changes to the relevant feature area under `src/features` and shared runtime/storage code under `src/shared` when needed.
- Scope native changes to `src-tauri/src/commands` and `src-tauri/src/services`.
- Preserve desktop behavior while treating Android as an active compatibility target.
- Prefer small wrappers around platform-specific behavior instead of branching feature logic across the app shell.
- Do not revert unrelated user changes already present in the worktree.

## Validation

Use the smallest relevant validation set for the change:

- `npm run build`
- `cd src-tauri && cargo check`
- `scripts/check-android-toolchain.sh`

For Android-specific work, also validate with:

- `npm run tauri:android:init -- --ci --skip-targets-install`
- `npx tauri android build --debug --apk --target aarch64 --ci`

## Current Product Constraints

- TDLib native login/downloads are expected to work in Tauri, including Android.
- Twitter/X native downloads are expected to work in Tauri, including Android.
- YouTube direct and signed stream downloads are supported on Android, but adaptive video-only formats are hidden there unless audio is already included.
- Opening or revealing files is desktop-first; Android may fall back to app/system paths where available.
- Service worker registration is disabled inside Tauri webviews by default.
