#!/usr/bin/env bash
set -euo pipefail

missing=()

has_library() {
  ldconfig -p 2>/dev/null | grep "$1" >/dev/null
}

if ! has_library 'libc++.so.1'; then
  missing+=('libc++.so.1')
fi

if ! has_library 'libc++abi.so.1'; then
  missing+=('libc++abi.so.1')
fi

if ! has_library 'libunwind.so.1'; then
  missing+=('libunwind.so.1')
fi

if [ "${#missing[@]}" -gt 0 ]; then
  printf 'TDLib runtime faltando: %s\n' "${missing[*]}" >&2
  printf 'Instale no Ubuntu/Debian com:\n' >&2
  printf '  sudo apt update && sudo apt install libc++1 libc++abi1 libunwind-18\n' >&2
  exit 1
fi
