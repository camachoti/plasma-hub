#!/usr/bin/env bash
set -euo pipefail

missing=()
notes=()

command_exists() {
  command -v "$1" >/dev/null 2>&1
}

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

android_clang_exists() {
  if command_exists aarch64-linux-android-clang; then
    return 0
  fi

  local sdk_dir="$1"
  if [ -n "$sdk_dir" ]; then
    find "$sdk_dir/ndk" -path '*/toolchains/llvm/prebuilt/*/bin/aarch64-linux-android*-clang' -type f -print -quit 2>/dev/null | grep -q .
  else
    return 1
  fi
}

if ! command_exists java; then
  missing+=('java')
fi

if ! command_exists javac; then
  missing+=('javac')
fi

sdk_dir="$(android_sdk_dir || true)"
if [ -z "$sdk_dir" ]; then
  missing+=('Android SDK (ANDROID_HOME or ANDROID_SDK_ROOT)')
else
  notes+=("Android SDK: $sdk_dir")
fi

if ! command_exists adb && { [ -z "$sdk_dir" ] || [ ! -x "$sdk_dir/platform-tools/adb" ]; }; then
  missing+=('adb / platform-tools')
fi

if [ -z "$sdk_dir" ] || [ ! -d "$sdk_dir/ndk" ]; then
  missing+=('Android NDK')
elif ! android_clang_exists "$sdk_dir"; then
  missing+=('aarch64-linux-android-clang')
fi

if [ "${#missing[@]}" -gt 0 ]; then
  printf 'Android toolchain incompleto: %s\n' "${missing[*]}" >&2
  printf '\nConfigure um JDK completo, Android SDK e NDK antes de rodar comandos Android.\n' >&2
  printf 'Exemplo:\n' >&2
  printf '  export ANDROID_HOME="$HOME/Android/Sdk"\n' >&2
  printf '  export ANDROID_SDK_ROOT="$ANDROID_HOME"\n' >&2
  printf '  export PATH="$ANDROID_HOME/platform-tools:$ANDROID_HOME/cmdline-tools/latest/bin:$PATH"\n' >&2
  printf '  sdkmanager "platform-tools" "platforms;android-35" "build-tools;35.0.0" "ndk;27.0.12077973"\n' >&2
  exit 1
fi

printf 'Android toolchain OK.\n'
printf '%s\n' "${notes[@]}"
