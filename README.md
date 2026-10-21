# Plasma Hub

Plasma Hub is a Tauri 2 application with a React/Vite frontend. The codebase is organized to keep the same app shell usable on desktop and Android, while platform-specific features are isolated behind small runtime and Tauri wrappers.

## Running

```bash
npm install
npm run tauri:dev
```

Build the desktop app:

```bash
npm run tauri:build
```

Initialize and run Android:

```bash
npm run tauri:android:init
npm run tauri:android:dev
```

`npm run tauri:android:build` targets `aarch64` by default. The Android migration targets modern arm64 Android devices (`arm64-v8a`).

Android support is an active compatibility target: the app should initialize and open, and native Telegram/TDLib plus Twitter/X download paths are expected to run through Tauri commands. YouTube uses the native Android extractor for direct streams and `signatureCipher` streams that can be resolved from the YouTube player.

Android builds require a full JDK, Android SDK, and Android NDK. If Gradle reports that the Java toolchain does not provide `JAVA_COMPILER`, install a JDK package that includes `javac` and point `JAVA_HOME` to it.

The Android npm scripts run `scripts/check-android-toolchain.sh` first. If it reports `aarch64-linux-android-clang` as missing, install an Android NDK through Android Studio or `sdkmanager`, then make sure `ANDROID_HOME` or `ANDROID_SDK_ROOT` points at the SDK.

## Project Map

- `Cargo.toml`: Cargo workspace for the Rust core and the Tauri app.
- `crates/plasma-core`: shared errors, app paths, and the UI-agnostic `AppEnv`.
- `crates/plasma-platform`: filesystem and OS helpers.
- `crates/plasma-store`: SQLite message cache, download history, topic state, settings, and migrations.
- `crates/plasma-download`: cancellation, streamed HTTPS downloads, and file sinks.
- `crates/plasma-media`: media cache helpers and video thumbnails.
- `crates/plasma-telegram`: TDLib integration and the loopback media server.
- `crates/plasma-twitter`, `crates/plasma-tiktok`, `crates/plasma-youtube`: native extractors and downloaders.
- `crates/plasma-duplicates`: duplicate media search.
- `crates/plasma-instagram`, `crates/plasma-reddit`: migration placeholders.
- `crates/plasma-ui`, `apps/plasma`: Rust UI migration placeholders.
- `src-tauri/src/commands`: thin Tauri adapters for the workspace crates.
- `src/app`: app shell, navigation, login flow, and bootstrap hooks.
- `src/features`: product domains such as Telegram, Twitter/X, downloader, and appearance.
- `src/shared/platform`: runtime capability detection, service worker registration, and typed Tauri wrappers.
- `src/shared/storage`: storage abstraction used before deciding the final desktop/mobile storage backend.

The domain crates are UI-agnostic: they never import `tauri`, `dioxus`, `tao` or `wry`.
See `docs/rust-rewrite-plan.md` for the full Rust migration plan.

## Platform Notes

The desktop app currently owns the full feature set. Android is intentionally conservative where native dependencies still need a mobile implementation:

- TDLib native login/downloads are enabled in Tauri, including Android.
- Twitter/X native downloads are enabled in Tauri, including Android, with download-directory fallback to app data when the system Downloads directory is unavailable.
- YouTube direct and signed stream downloads can be saved by the native downloader on Android. Adaptive video-only formats are hidden on Android unless they already include audio, because the mobile path does not mux separate audio/video tracks.
- Opening/revealing files is desktop-first; Android falls back to app/system paths where available.
- Service worker registration is disabled inside Tauri webviews by default.

## Validation

Useful checks while refactoring:

```bash
npm run build
npm run check:rust
cargo check -p plasma-hub
scripts/check-android-toolchain.sh
```

Validate the generated Android project and native library with:

```bash
npm run tauri:android:init -- --ci --skip-targets-install
npx tauri android build --debug --apk --target aarch64 --ci
```

Use `--target aarch64` for physical Android validation. Other Android ABIs are outside the current migration scope.

The debug APK is emitted at `src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk`.
Install the release APK on a connected device or emulator with:

```bash
npm run tauri:android:install
```

The install script uses `adb` from `ANDROID_HOME`, `ANDROID_SDK_ROOT`, or `$HOME/Android/Sdk` when `platform-tools` is not on `PATH`.

Use the Tauri Android command for mobile validation instead of plain `cargo check --target ...`, because the Tauri CLI injects the Android NDK linker environment expected by native dependencies.

If local builds fail with `No space left on device`, clear generated Cargo/Tauri artifacts:

```bash
cd src-tauri && cargo clean
```

For repeated native builds on a small partition, use a target directory on a larger disk:

```bash
CARGO_TARGET_DIR=/path/with/space/plasma-target npm run tauri:dev
```

Manual desktop regression areas:

- Login and skip-login.
- Telegram navigation, messages, sending, and media download.
- Downloads tab and Twitter/X library.
- Settings and appearance changes.
