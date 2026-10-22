#!/usr/bin/env bash
# Builds the Dioxus Android shell and packages TDLib into the APK.
#
# `dx` links `libmain.so` against `libtdjson.so` but does not copy the latter
# into the generated Gradle project. This script performs that packaging step
# and then invokes Gradle again so the APK is actually installable.
set -euo pipefail

mode="${1:-debug}"
if [[ "$mode" != "debug" && "$mode" != "release" ]]; then
  printf 'Uso: %s [debug|release]\n' "$0" >&2
  exit 2
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
app_dir="$repo_root/apps/plasma"
target="aarch64-linux-android"

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

sdk_dir="$(android_sdk_dir || true)"
if [ -z "$sdk_dir" ]; then
  printf 'Android SDK não encontrado. Defina ANDROID_HOME.\n' >&2
  exit 1
fi

ndk_dir="${ANDROID_NDK_HOME:-${NDK_HOME:-}}"
if [ -z "$ndk_dir" ]; then
  ndk_dir="$(find "$sdk_dir/ndk" -mindepth 1 -maxdepth 1 -type d 2>/dev/null | sort -V | tail -1)"
fi
if [ -z "$ndk_dir" ] || [ ! -d "$ndk_dir" ]; then
  printf 'Android NDK não encontrado. Defina ANDROID_NDK_HOME.\n' >&2
  exit 1
fi

if ! command -v dx >/dev/null 2>&1; then
  if [ -n "${DX:-}" ] && [ -x "$DX" ]; then
    dx_bin="$DX"
  else
    printf 'dx (Dioxus CLI 0.7.10) não encontrado. Instale com:\n' >&2
    printf '  cargo install dioxus-cli --version 0.7.10 --locked\n' >&2
    exit 1
  fi
else
  dx_bin="$(command -v dx)"
fi

export ANDROID_HOME="$sdk_dir"
export ANDROID_SDK_ROOT="$sdk_dir"
export ANDROID_NDK_HOME="$ndk_dir"
export NDK_HOME="$ndk_dir"

cd "$app_dir"

dx_args=(build --android --target "$target" -p plasma)
if [ "$mode" = "release" ]; then
  dx_args+=(--release)
fi
"$dx_bin" "${dx_args[@]}"

profile_dir="debug"
gradle_task="assembleDebug"
if [ "$mode" = "release" ]; then
  profile_dir="release"
  gradle_task="assembleRelease"
fi

tdlib_lib="$(find "$repo_root/target/$target" "$app_dir/target/$target" \
  -path "*/build/tdlib-rs-*/out/tdlib/lib/libtdjson.so" -print -quit 2>/dev/null || true)"
if [ -z "$tdlib_lib" ] || [ ! -f "$tdlib_lib" ]; then
  printf 'libtdjson.so não encontrado para %s.\n' "$target" >&2
  exit 1
fi

# O app pertence ao workspace raiz, então o dx grava em target/dx/plasma;
# o fallback cobre um dx antigo que ainda use o target local do app.
gradle_project="$(find "$repo_root/target/dx/plasma/$profile_dir/android/app" \
  "$app_dir/target/dx/plasma/$profile_dir/android/app" \
  -maxdepth 0 -type d -print -quit 2>/dev/null || true)"
if [ -z "$gradle_project" ] || [ ! -d "$gradle_project" ]; then
  printf 'Projeto Android gerado não encontrado para o perfil %s.\n' "$profile_dir" >&2
  exit 1
fi
jni_dir="$gradle_project/app/src/main/jniLibs/arm64-v8a"
if [ ! -d "$jni_dir" ]; then
  printf 'Projeto Android gerado não encontrado em %s.\n' "$jni_dir" >&2
  exit 1
fi
cp "$tdlib_lib" "$jni_dir/libtdjson.so"

(
  cd "$gradle_project"
  ./gradlew "$gradle_task"
)

apk="$(find "$gradle_project/app/build/outputs/apk/$profile_dir" -name '*.apk' -print -quit 2>/dev/null || true)"
if [ -n "$apk" ] && [ -f "$apk" ]; then
  printf '\nAPK: %s\n' "$apk"
else
  printf '\nAPK gerado, mas não encontrado em %s.\n' "$gradle_project/app/build/outputs/apk/$profile_dir" >&2
fi
