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

cd "$app_dir"

# 1. Compila o release e localiza a libtdjson baixada pelo tdlib-rs.
cargo build --release
tdlib_lib="$(find target/release/build -path '*/tdlib-rs-*/out/tdlib/lib/libtdjson.so.1.8.61' -print -quit)"
if [[ -z "$tdlib_lib" ]]; then
    printf 'libtdjson.so.1.8.61 não encontrado em target/release/build.\n' >&2
    exit 1
fi

# O dx lê [bundle.deb.files] no início do bundle, então a lib precisa existir
# antes da chamada.
mkdir -p packaging/linux
cp -L "$tdlib_lib" packaging/linux/libtdjson.so.1.8.61

# 2. .deb. O APPIMAGE/RPM podem ser adicionados quando houver ferramenta
# (appimagetool/rpmbuild) disponível.
dx bundle --desktop --release --package-types deb

# 3. Diretório portátil (rpath $ORIGIN).
rm -rf "$portable_dir"
mkdir -p "$portable_dir"
cp target/release/plasma "$portable_dir/plasma"
cp -L "$tdlib_lib" "$portable_dir/libtdjson.so.1.8.61"

# 4. Reúne os artefatos em dist/, removendo restos do frontend Vite antigo.
mkdir -p "$dist_dir"
rm -rf "$dist_dir/assets" "$dist_dir/index.html" "$dist_dir/sw.js" \
    "$dist_dir/tauri.svg" "$dist_dir/vite.svg"
deb_dir="target/dx/plasma/bundle/linux/deb"
deb="$(find "$deb_dir" -maxdepth 1 -type f -name '*.deb' -print -quit)"
if [[ -z "$deb" ]]; then
    printf 'Nenhum .deb encontrado em %s.\n' "$deb_dir" >&2
    exit 1
fi
cp "$deb" "$dist_dir/"

printf '\nArtefatos em %s\n' "$dist_dir"
ls -lh "$dist_dir"
ls -lh "$portable_dir"
