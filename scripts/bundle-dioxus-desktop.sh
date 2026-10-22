#!/usr/bin/env bash
# Empacota o shell Dioxus para desktop Linux.
#
# Gera o .deb com o dx (que instala o binário em /usr/bin/plasma e o
# libtdjson em /usr/lib/plasma) e um diretório portátil com a biblioteca ao
# lado do binário, para smoke test sem instalar o pacote.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
app_dir="$repo_root/apps/plasma"
dist_dir="$repo_root/dist"
portable_dir="$dist_dir/plasma-linux-x86_64"

if ! command -v dx >/dev/null 2>&1; then
    printf 'dx (Dioxus CLI 0.7.10) não encontrado.\nInstale com:\n  cargo install dioxus-cli --version 0.7.10 --locked\n' >&2
    exit 1
fi

# 1. Compila o release no workspace raiz e localiza a libtdjson do tdlib-rs.
cd "$repo_root"
cargo build --release -p plasma
tdlib_lib="$(find "$repo_root/target/release/build" -path '*/tdlib-rs-*/out/tdlib/lib/libtdjson.so.1.8.61' -print -quit)"
if [[ -z "$tdlib_lib" ]]; then
    printf 'libtdjson.so.1.8.61 não encontrado em target/release/build.\n' >&2
    exit 1
fi

# O dx lê [bundle.deb.files] no início do bundle, então a lib precisa existir
# antes da chamada (o caminho no Dioxus.toml é relativo a apps/plasma).
mkdir -p "$app_dir/packaging/linux"
cp -L "$tdlib_lib" "$app_dir/packaging/linux/libtdjson.so.1.8.61"

# 2. .deb. O APPIMAGE/RPM podem ser adicionados quando houver ferramenta
# (appimagetool/rpmbuild) disponível. O dx roda a partir do app para ler o
# Dioxus.toml; como o app agora pertence ao workspace raiz, os artefatos vão
# para target/dx/plasma (com fallback para o target local do app).
cd "$app_dir"
dx bundle --desktop --release --package-types deb

# 3. Diretório portátil (rpath $ORIGIN).
rm -rf "$portable_dir"
mkdir -p "$portable_dir"
cp "$repo_root/target/release/plasma" "$portable_dir/plasma"
cp -L "$tdlib_lib" "$portable_dir/libtdjson.so.1.8.61"

# 4. Reúne os artefatos em dist/, removendo restos do frontend Vite antigo.
mkdir -p "$dist_dir"
rm -rf "$dist_dir/assets" "$dist_dir/index.html" "$dist_dir/sw.js" \
    "$dist_dir/tauri.svg" "$dist_dir/vite.svg"
deb="$(find "$repo_root/target/dx/plasma/bundle/linux/deb" \
    -maxdepth 1 -type f -name '*.deb' -print -quit 2>/dev/null || true)"
if [[ -z "$deb" ]]; then
    deb="$(find "$app_dir/target/dx/plasma/bundle/linux/deb" \
        -maxdepth 1 -type f -name '*.deb' -print -quit 2>/dev/null || true)"
fi
if [[ -z "$deb" ]]; then
    printf 'Nenhum .deb encontrado em target/dx/plasma/bundle/linux/deb.\n' >&2
    exit 1
fi
cp "$deb" "$dist_dir/"

printf '\nArtefatos em %s\n' "$dist_dir"
ls -lh "$dist_dir"
ls -lh "$portable_dir"
