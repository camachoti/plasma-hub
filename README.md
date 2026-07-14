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

Android support is currently an initial compatibility target: the app should initialize and open, while desktop-only capabilities can be gated until they receive a mobile implementation.

Android builds require a full JDK, Android SDK, and Android NDK. If Gradle reports that the Java toolchain does not provide `JAVA_COMPILER`, install a JDK package that includes `javac` and point `JAVA_HOME` to it.

## Project Map

- `src/app`: app shell, navigation, login flow, and bootstrap hooks.
- `src/features`: product domains such as Telegram, Twitter/X, downloader, and appearance.
- `src/shared/platform`: runtime capability detection, service worker registration, and typed Tauri wrappers.
- `src/shared/storage`: storage abstraction used before deciding the final desktop/mobile storage backend.
- `src-tauri/src/commands`: commands exposed to the webview.
- `src-tauri/src/services`: native services for TDLib, Twitter/X, YouTube, and Android fallbacks.

## Platform Notes

The desktop app currently owns the full feature set. Android is intentionally conservative until each native dependency is validated:

- TDLib native login/downloads are desktop-first; Android commands return controlled unavailable responses.
- YouTube downloads depend on `yt-dlp`, which is desktop-first; Android commands return controlled unavailable responses.
- Twitter/X native downloads are desktop-first; Android commands return controlled unavailable responses.
- Opening/revealing files and system download directories are desktop-first.
- Service worker registration is disabled inside Tauri webviews by default.

## Validation

Useful checks while refactoring:

```bash
npm run build
cd src-tauri && cargo check
```

Validate the generated Android project and native library with:

```bash
npm run tauri:android:init -- --ci --skip-targets-install
npx tauri android build --debug --apk --target aarch64 --ci
```

Use `--target aarch64` for most physical Android devices. Use `--target x86_64` for x86_64 emulators.

The debug APK is emitted at `src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk`.
Install it on a connected device or emulator with:

```bash
adb install -r src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk
```

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
