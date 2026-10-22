# Plasma Hub

Cliente de Telegram para desktop e Android com baixador de mídia (YouTube,
Twitter/X, TikTok, Instagram, Reddit e links diretos). O aplicativo é escrito em
Rust, com UI em Dioxus e um núcleo de domínio independente de UI.

## Tecnologias

| Camada | Tecnologia |
| --- | --- |
| Linguagem | Rust 2021 (MSRV 1.85) |
| UI desktop | Dioxus 0.7.10 (WebView via wry + WebKitGTK 4.1) |
| UI Android | Dioxus 0.7.10 (mobile) + JNI + Gradle |
| Telegram | TDLib via `tdlib-rs` (`libtdjson` compartilhado) |
| Persistência | SQLite via `rusqlite` (bundled), WAL e migrações |
| Async | Tokio |
| Rede/downloads | `reqwest` + extratores nativos por plataforma |
| Mídia | cache próprio; thumbnails com `ffmpeg` no desktop e `MediaMetadataRetriever` no Android |
| Estilo | CSS embutido (`include_str!`) e ícones SVG próprios |
| Empacotamento | Dioxus CLI 0.7.10 (`dx bundle`) → `.deb` + diretório portátil |
| Frontend legado | React/TypeScript em `reference/react-frontend/` (somente consulta; não é compilado) |

O build, os testes e o empacotamento não dependem de Node/npm: usam apenas Cargo
e, para empacotar, o Dioxus CLI.

## Requisitos

### Desktop Linux

- Rust 1.85 ou mais novo (recomendado: rustup).
- GTK 3, WebKitGTK 4.1, libsoup 3 e as bibliotecas C++ usadas pelo TDLib.

No Ubuntu/Zorin:

```bash
sudo apt install build-essential pkg-config libgtk-3-dev \
  libwebkit2gtk-4.1-dev libsoup-3.0-dev libssl-dev libxdo-dev \
  libc++1 libc++abi1 libunwind-18
```

`ffmpeg` é necessário para gerar thumbnails de vídeo no desktop. O `dx`
(Dioxus CLI 0.7.10) só é necessário para empacotar e para o Android:

```bash
cargo install dioxus-cli --version 0.7.10 --locked
```

### Credenciais do Telegram

O build embute `PLASMA_TELEGRAM_API_ID` / `PLASMA_TELEGRAM_API_HASH` (ou
`VITE_API_ID` / `VITE_API_HASH`) lidos do ambiente ou do `.env` na raiz do
projeto. Variáveis de ambiente em tempo de execução têm precedência. O `.env`
não é versionado.

## Rodando

```bash
cargo run -p plasma
```

Na primeira execução o app cria os diretórios de dados e o
`plasma-hub.sqlite3`. Para isolar dados de desenvolvimento ou testes:

```bash
PLASMA_HOME=/tmp/plasma-dev cargo run -p plasma
```

`PLASMA_HOME` é uma opção de desenvolvimento; sem ela, os caminhos seguem o
diretório de dados da plataforma (identificador `com.camacho.plasmahub`).

Build otimizado:

```bash
cargo build -p plasma --release
```

## Checks

Validação completa do workspace:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo check --workspace --all-targets
```

Durante o desenvolvimento, os recortes mais rápidos são:

```bash
cargo check -p plasma
cargo test -p plasma-ui
cargo clippy -p plasma-ui --all-targets -- -D warnings
```

## Empacotamento desktop

```bash
scripts/bundle-dioxus-desktop.sh
```

O script:

1. compila o app em `--release`;
2. copia o `libtdjson.so.1.8.61` baixado pelo `tdlib-rs` para
   `apps/plasma/packaging/linux/`;
3. roda `dx bundle --desktop --release --package-types deb`;
4. monta `dist/plasma-linux-x86_64/` com o binário e a biblioteca ao lado
   (RUNPATH `$ORIGIN`).

Artefatos:

- `dist/plasma_0.1.0_amd64.deb` — binário em `/usr/bin/plasma` e `libtdjson` em
  `/usr/lib/plasma` (RUNPATH `$ORIGIN/../lib/plasma`);
- `dist/plasma-linux-x86_64/plasma` — execução direta sem instalar.

Para instalar o pacote Debian:

```bash
sudo apt install ./dist/plasma_0.1.0_amd64.deb
```

## Android

Requisitos: JDK com `javac`, Android SDK, Android NDK e Dioxus CLI 0.7.10.

```bash
scripts/check-android-toolchain.sh
scripts/build-dioxus-android.sh debug
```

O script roda `dx build --android --target aarch64-linux-android`, copia o
`libtdjson.so` para o `jniLibs/arm64-v8a` do projeto gerado e chama o Gradle —
o `dx` sozinho não empacota essa biblioteca. O APK fica em
`target/dx/plasma/debug/android/app/app/build/outputs/apk/debug/`.

Para instalar em um device/emulador conectado:

```bash
scripts/install-android-apk.sh
```

O script usa o `adb` de `ANDROID_HOME`, `ANDROID_SDK_ROOT` ou
`$HOME/Android/Sdk`.

## Estrutura do repositório

- `Cargo.toml`: workspace Cargo com todos os crates, o shell e o app.
- `crates/plasma-core`: erros, caminhos do app, `AppEnv` e eventos.
- `crates/plasma-platform`: helpers de filesystem e sistema operacional.
- `crates/plasma-store`: cache SQLite de mensagens, histórico de downloads,
  tópicos, settings e migrações.
- `crates/plasma-download`: cancelamento, downloads HTTPS em streaming e sinks.
- `crates/plasma-extract`: análise/despacho unificado por plataforma.
- `crates/plasma-media`: cache de mídia e thumbnails.
- `crates/plasma-telegram`: integração TDLib e servidor HTTP local de mídia.
- `crates/plasma-twitter`, `plasma-tiktok`, `plasma-youtube`: extratores e
  downloaders nativos.
- `crates/plasma-duplicates`: busca de mídia duplicada.
- `crates/plasma-instagram`, `plasma-reddit`: placeholders da migração.
- `crates/plasma-ui`: views Dioxus, tema, timeline virtualizada e CSS.
- `apps/plasma`: composition root desktop/Android e renderer Dioxus.
- `reference/react-frontend`: código React/TypeScript mantido como referência
  para funcionalidades ainda não portadas. Não é compilado nem testado.
- `docs/rust-rewrite-plan.md`: plano e histórico da migração para Rust.
- `scripts/bundle-dioxus-desktop.sh`: empacotamento desktop.
- `scripts/build-dioxus-android.sh`: empacotamento Android.
- `scripts/check-android-toolchain.sh`: validação do ambiente Android.

Os crates de domínio são independentes de UI: não importam `dioxus`, `tao` ou
`wry`. Eles recebem `plasma_core::AppEnv` e devolvem tipos de domínio; o shell
apenas renderiza e encaminha ações.

## Dados e compatibilidade

- O SQLite continua sendo `plasma-hub.sqlite3`, com o mesmo schema e as mesmas
  migrações do app anterior.
- Cookies, mídia baixada e dados do TDLib permanecem nos caminhos existentes.
- `.env`, keystores e arquivos de assinatura não são versionados.

## Notas de plataforma

- TDLib (login, mensagens e downloads) funciona no desktop e no Android.
- Twitter/X usa o downloader nativo nas duas plataformas, com fallback para o
  diretório de dados do app quando o diretório de Downloads do sistema não está
  disponível.
- YouTube aceita streams diretos e assinados no Android; formatos adaptativos
  só de vídeo ficam ocultos porque o caminho mobile não faz mux de áudio/vídeo.
- Abrir/mostrar arquivos é prioridade desktop; no Android o app cai para o
  próprio arquivo ou caminhos do sistema quando não há equivalente.
- O Android usa `network_security_config` com cleartext liberado apenas para
  `127.0.0.1` (playback e thumbnails via servidor loopback).

## Problemas comuns

- **`libtdjson.so.1.8.61: cannot open shared object file`**: use o diretório
  portátil/`.deb` gerado pelo script, ou rode a partir do repositório (o build
  adiciona o caminho absoluto do `tdlib-rs` ao RUNPATH).
- **Janela em branco no desktop**: valide o WebKitGTK e o renderer do Dioxus
  conforme os riscos em `docs/rust-rewrite-plan.md`.
- **Pouco espaço em disco**: `cargo clean` remove os artefatos; o diretório
  `target/` pode ser apontado para outro disco com
  `CARGO_TARGET_DIR=/caminho/maior/plasma-target`.
