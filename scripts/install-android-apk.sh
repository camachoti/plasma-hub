#!/usr/bin/env bash
set -euo pipefail

apk_path="${1:-src-tauri/gen/android/app/build/outputs/apk/universal/release/app-universal-release-unsigned.apk}"

android_sdk_dir() {
  if [ -n "${ANDROID_HOME:-}" ] && [ -d "${ANDROID_HOME}" ]; then
    printf '%s\n' "${ANDROID_HOME}"
    return
  fi

  if [ -n "${ANDROID_SDK_ROOT:-}" ] && [ -d "${ANDROID_SDK_ROOT}" ]; then
    printf '%s\n' "${ANDROID_SDK_ROOT}"
    return
  fi

  for candidate in "$HOME/Android/Sdk" "$HOME/android-sdk" "/opt/android-sdk"; do
    if [ -d "$candidate" ]; then
      printf '%s\n' "$candidate"
      return
    fi
  done
}

adb_bin="$(command -v adb || true)"
sdk_dir="$(android_sdk_dir || true)"

if [ -z "$adb_bin" ] && [ -n "$sdk_dir" ] && [ -x "$sdk_dir/platform-tools/adb" ]; then
  adb_bin="$sdk_dir/platform-tools/adb"
fi

if [ -z "$adb_bin" ]; then
  printf 'adb nao encontrado. Instale Android platform-tools ou exporte ANDROID_HOME/ANDROID_SDK_ROOT.\n' >&2
  exit 1
fi

if [ ! -f "$apk_path" ]; then
  printf 'APK nao encontrado: %s\n' "$apk_path" >&2
  printf 'Gere primeiro com: npm run tauri:android:build\n' >&2
  exit 1
fi

if ! "$adb_bin" get-state >/dev/null 2>&1; then
  printf 'Nenhum dispositivo/emulador Android conectado ou autorizado.\n' >&2
  printf 'Conecte o aparelho, habilite depuracao USB e aceite a autorizacao ADB.\n' >&2
  "$adb_bin" devices >&2 || true
  exit 1
fi

"$adb_bin" install -r "$apk_path"
