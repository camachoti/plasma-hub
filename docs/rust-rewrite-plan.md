# Plasma Hub 100% Rust — Plano de Reescrita

> Status: **aprovado**. Fases 0–7 implementadas; foco atual no desktop antes da validação Android.
> Escopo: reescrever o app inteiro em Rust (UI + lógica), removendo React/Vite/Tauri IPC.
>
> **Execução atual:** o shell Dioxus desktop foi validado com TDLib real e o gate visual
> foi desbloqueado em 2026-10-21. A tela preta era causada por um cache WebKitGTK parcial;
> o app agora isola e reinicia o cache do WebView a cada execução. O download em massa de
> Telegram ganhou painel no shell Rust (pasta, tópico, divisão por usuário/álbum, progresso
> detalhado e parada). O frontend React continua sendo o app padrão até a Fase 8.
> Próximos passos desktop: validação em conta real, empacotamento e cutover (Fase 8).

---

## 1. Resumo executivo

O backend já é substancialmente Rust (~10.000 linhas em 14 arquivos) e concentra o que é
caro: TDLib (4.645 linhas), Twitter/X (1.780), YouTube (645), TikTok (606), cache SQLite
(484), filesystem/downloads (483), busca de duplicados (658). O que falta para "tudo em
Rust" é a **camada de aplicação e UI**: 72 arquivos TS/TSX (~16.700 linhas) + 7.688 linhas
de CSS, hoje falando com o backend por 61 comandos Tauri e por `localStorage`.

**Decisão recomendada:** reescrever a UI em **Dioxus 0.7.10 usando o renderer WebView**
(wry/tao — a mesma base do Tauri), com um **workspace de crates** para o domínio e
**cutover por fases**, mantendo o app Tauri atual funcionando até haver paridade.

Por quê essa stack:

- Mantém HTML/CSS → reaproveitamos ~7.700 linhas de CSS e o design system quase 1:1;
- Mantém `<video>`/`<audio>` do WebView → o playback de mídia do Telegram (que hoje depende
  do protocolo `plasma-media://` + servidor HTTP local) continua viável sem escrever um
  player nativo;
- Android já é suportado pelo renderer WebView do Dioxus (o renderer nativo/Blitz ainda é
  experimental em 2026);
- TDLib, reqwest, rusqlite, axum etc. continuam exatamente como estão;
- Não há IPC: a UI chama o domínio diretamente, com tipos Rust de ponta a ponta.

**Esforço estimado:** 16–24 semanas de trabalho solo, +25–30% de contingência
(≈ 20–31 semanas). É uma reescrita, não um refactor. O ganho principal é eliminar a
duplicação TS↔Rust e o custo de manter duas linguagens; o custo é alto e o risco
concentra-se em virtualização de listas, mídia no Android e maturidade do Dioxus.

Se o objetivo for "Rust em todo o produto" com menor risco e 80% do benefício, existe o
plano B (Seção 11): manter Tauri e mover toda a lógica restante para Rust. Ele não foi o
escolhido, mas deve ficar registrado como fallback.

---

## 2. Estado atual (fotografia)

| Camada | Tamanho | Observações |
|---|---:|---|
| Rust (`src-tauri/src`) | ~10.022 linhas | 61 comandos Tauri, 8 serviços, SQLite, servidor HTTP local |
| TS/TSX (`src`) | ~16.741 linhas | 72 arquivos; UI + orquestração + extractors |
| CSS (`src/styles`) | 7.688 linhas | 8 arquivos; reutilizável no renderer WebView |
| Testes | 2 arquivos `.mjs` | `downloader-utils`, `download-service` |

Distribuição relevante:

- `Dashboard.tsx` 3.917 linhas; `TelegramService.ts` 2.512; `MessageMedia.tsx` 834;
  `MediaCacheService.ts` 788; `Downloads.tsx` 562; `TelegramTdlibBridge.ts` 472;
  `downloader.ts` 457; `Settings.tsx` 453; `TwitterLibrary.tsx` 382.
- Rust: `telegram.rs` 4.645; `twitter.rs` 1.780; `youtube.rs` 645; `tiktok.rs` 606;
  `duplicate_search.rs` 658; `filesystem.rs` 483; `message_cache.rs` 484;
  `media_http.rs` 338.

Persistência hoje:

- `localStorage` (`appStorage`): `skip_login`, paleta/densidade, cookies do Twitter,
  histórico de downloads (cópia), configurações de cache.
- Rust: `plasma-hub.sqlite3` em `app_data_dir` (cache de mensagens, histórico de downloads,
  estado de mass download por tópico), arquivos de mídia em `app_cache_dir`, cookies do
  Twitter em arquivo.
- TDLib gerencia a própria sessão/DB.

Plugins Tauri em uso: `dialog`, `opener`, `http`, protocolo customizado `plasma-media`,
protocolo `asset:`. Todos têm equivalente Rust direto (`rfd`, `opener`, `reqwest`,
servidor axum em loopback que **já existe** em `services/telegram/media_http.rs`).

### O que já é Rust e será apenas movido

`services/telegram.rs`, `services/telegram/media_http.rs`, `services/twitter.rs`,
`services/youtube.rs`, `services/tiktok.rs`, `services/message_cache.rs`,
`services/download_control.rs`, `commands/filesystem.rs`, `commands/duplicate_search.rs`,
`fontconfig_guard.rs`.

### O que será reescrito em Rust

- Todos os `.tsx` (views e componentes).
- `TelegramService.ts` e `TelegramTdlibBridge.ts` (orquestração, throttling, eventos).
- `MediaCacheService.ts` e `TelegramFileStorage.ts`.
- `DownloadService.ts`, `downloader.ts`, `downloaderApi.ts`.
- `extractors/*` (YouTube/TikTok já têm versão nativa; Instagram e Reddit só existem em TS).
- Stores (`AppearanceStore`, `TwitterSettingsStore`), `appStorage`, `shared/platform/*`,
  `shared/debug/logger.ts`, i18n inline.

---

## 3. Stack de UI: decisão e alternativas

| Opção | Desktop | Android | Vídeo | Reuso de CSS/HTML | Veredito |
|---|---|---|---|---|---|
| **Dioxus 0.7.10 (WebView)** | Estável (wry/tao) | Estável (dx + Gradle) | `<video>` nativo do WebView | Total | **Escolhida** |
| Tauri 2 + Rust (status quo) | Estável | Estável | Sim | Total | Mantém JS/TS; não atende "tudo em Rust" |
| Dioxus 0.8 alpha / Blitz | Alpha | Experimental | Precisa de widget próprio | Parcial | Não usar agora |
| Slint 1.18 | Estável | Estável (android-activity) | Sem player: exige GStreamer/ExoPlayer via JNI | Nenhum (DSL `.slint`) | Melhor visual nativo, pior para mídia |
| egui/eframe | Estável | Suportado, porém IME/acessibilidade fracos | Textura + decoder próprio | Nenhum | Ruim para chat denso |
| Iced | Estável | Não é alvo de primeira linha | Widget custom | Nenhum | Não recomendado |
| Xilem/Masonry | Experimental | Não | — | Nenhum | Fora de escopo |

Ponto crítico: o app é **media-heavy** (fotos, álbuns, vídeos, áudio, lightbox,
miniaturas, cache, streaming com `Range`). O WebView resolve isso de graça; qualquer
toolkit de renderização nativa exigiria integrar GStreamer (Linux), Media Foundation
(Windows), AVFoundation (macOS) e ExoPlayer/MediaPlayer via JNI (Android), além de
sincronizar superfícies de vídeo com o renderer. Isso transformaria a reescrita em dois
projetos.

**Versão fixada:** `dioxus = "=0.7.10"` (última estável, jul/2026). `0.8.0-alpha.*` só
depois do cutover e em um spike isolado. `dx` CLI para dev/mobile; desktop também compila
com `cargo`.

---

## 4. Arquitetura alvo (workspace)

```
plasma-hub/
├── Cargo.toml                    # [workspace] members = ["crates/*", "apps/*"]
├── crates/
│   ├── plasma-core/              # erros, IDs, config, DTOs serde, tempo, logging
│   ├── plasma-platform/          # paths, dialogs, opener, clipboard, share intent, drag-drop, JNI
│   ├── plasma-store/             # SQLite (rusqlite) + migrations + settings + cookies + histórico
│   ├── plasma-media/             # dirs/metadata de mídia, thumbnails, cache stats/eviction, servidor axum
│   ├── plasma-download/          # job manager, progresso, cancelamento, retry, sinks, URL download
│   ├── plasma-telegram/          # TDLib: auth FSM, chats, mensagens, envio, mídia, mass download
│   ├── plasma-twitter/           # GraphQL/scraping, cookies, perfil/tweet, download
│   ├── plasma-tiktok/
│   ├── plasma-youtube/           # extração web + player, signatureCipher, yt-dlp (desktop)
│   ├── plasma-instagram/         # hoje só existe em TS
│   ├── plasma-reddit/            # hoje só existe em TS
│   ├── plasma-duplicates/        # hash perceptual + busca
│   ├── plasma-ui/                # app Dioxus: components, views, tema, ícones, assets CSS
│   └── plasma-testkit/           # fixtures (yt_test_*.json), helpers de teste
├── apps/
│   └── plasma/                   # composition root: desktop main + android entry
├── assets/
│   ├── styles/                   # CSS migrado de src/styles
│   └── icons/                    # SVGs Tabler usados (codegen para Rust)
└── docs/
```

### Regras de dependência (aplicar no CI)

```
plasma-core  ←  tudo
plasma-platform ← serviços que precisam de paths/OS
plasma-store / plasma-media / plasma-download ← serviços e UI
plasma-telegram / plasma-twitter / plasma-youtube / ... ← plasma-ui e apps/plasma
plasma-ui    ←  apps/plasma
```

- Nenhum crate de domínio importa `dioxus`, `tao`, `wry` ou `tauri`.
- Nenhum crate de domínio recebe `AppHandle`/`Window`; recebe `Arc<AppServices>`.
- `apps/plasma` é o único lugar que conhece janela, tema do SO, ciclo de vida Android.

> **Workspace aninhado (fases 2–7):** o Dioxus 0.7.10 usa `wry 0.53.5`, que fixa
> `webkit2gtk =2.0.1`; o Tauri 2.12 usa `wry 0.57`, que fixa `=2.0.2`. Como o Cargo não
> consegue unificar duas versões exatas da mesma biblioteca de sistema, `apps/plasma` tem
> um workspace próprio (`[workspace]`) e a raiz o exclui de `members`. `crates/plasma-ui`
> permanece no workspace raiz porque só usa o núcleo do Dioxus (sem renderer/wry). Na fase 8,
> ao remover o Tauri, `apps/plasma` volta para o workspace raiz e os lockfiles são unificados.

### Composição em runtime

```rust
pub struct AppServices {
    pub paths: Arc<AppPaths>,
    pub settings: Arc<SettingsStore>,
    pub db: Arc<Db>,
    pub events: EventBus,          // tokio::sync::broadcast<AppEvent>
    pub media: Arc<MediaService>,  // inclui o servidor loopback
    pub downloads: Arc<DownloadManager>,
    pub telegram: Arc<TelegramService>, // lazy: só conecta após init
    pub twitter: Arc<TwitterService>,
    pub youtube: Arc<YouTubeService>,
    pub tiktok: Arc<TikTokService>,
}
```

`AppEvent` substitui os `CustomEvent`/`listen()` de hoje:

```rust
pub enum AppEvent {
    TelegramAuth(AuthState),
    TelegramNewMessage { chat_id: i64, topic: Option<TopicRef>, message: Message },
    TelegramMessagesDeleted { chat_id: i64, ids: Vec<i64> },
    MediaProgress(MediaProgress),
    DownloadChanged(DownloadItem),
    DownloadBatchProgress(BatchProgress),
    SearchProgress(SearchProgress),
    Log(LogEntry),
}
```

A UI assina com `use_future`/`use_coroutine` e atualiza signals. Eventos de alta
frequência (progresso de mídia/download) são **coalescidos** (ex.: no máximo 10 Hz por
chave), como já é feito hoje no TS.

### Erros

- `plasma-core::Error` com `thiserror`; variantes por domínio (`Telegram`, `Http`,
  `Io`, `Db`, `Cancelled`, `Config`).
- `Result<T, Error>` em toda a API interna; nada de `Result<T, String>` (o código atual
  usa `String` em 61 comandos — será convertido nas bordas).
- `Cancelled` é variante própria para não confundir com falha real (hoje é comparado por
  substring em `download_control.rs`).

### Logging

Substituir `useWebviewLogger`/`debugLog` por `tracing` + `tracing-subscriber` com rotação
em `app_log_dir`. O painel de logs do Settings vira um stream de `AppEvent::Log`.

---

## 5. UI (plasma-ui)

### Estrutura

```
plasma-ui/src/
├── app.rs                 # Shell, abas, modais, command palette, toasts
├── theme.rs               # 6 paletas × 3 densidades (tokens CSS)
├── state/                 # AppState, selectors, sinais globais
├── components/            # design system (Button, TextField, Select, Dialog, EmptyState,
│                          #   Loading, ContextMenu, Skeletons, Icon)
├── views/
│   ├── login/
│   ├── telegram/          # chat_list, timeline, composer, info_panel, mass_download,
│   │                      #   duplicate_search, media (preview/lightbox/player)
│   ├── downloads/
│   ├── twitter/
│   └── settings/
├── icons.rs               # gerado a partir dos SVGs Tabler usados
└── assets/                # CSS (include_str!/manganis)
```

### Mapa de componentes

| Hoje | Amanhã |
|---|---|
| `src/design-system/*` | `plasma-ui/components/*` + mesmo CSS |
| `src/components/MessageMedia*.tsx` | `plasma-ui/views/telegram/media/*` |
| `src/app/AppShell.tsx` | `plasma-ui/app.rs` |
| `src/app/LoginScreen.tsx` | `plasma-ui/views/login.rs` |
| `src/features/telegram/Dashboard*.tsx` (≈10 arquivos) | `plasma-ui/views/telegram/*` |
| `src/features/telegram/Downloads.tsx` | `plasma-ui/views/downloads.rs` |
| `src/features/telegram/Settings.tsx` | `plasma-ui/views/settings.rs` |
| `src/features/twitter/TwitterLibrary.tsx` | `plasma-ui/views/twitter.rs` |
| `src/design-system/icons.ts` + `@tabler/icons-react` | codegen `icons.rs` (SVG inline) |

Os 7.688 linhas de CSS são reaproveitadas com ajustes pontuais (remoção de
`-webkit-app-region`, seletores de portal, etc.). O codegen de ícones elimina o único uso
real de dependência npm além do React.

### Listas virtualizadas (risco alto)

`react-virtuoso` hoje lida com alturas variáveis, ancoragem no fundo e carregamento
bidirecional. Em Rust:

1. **Spike na Fase 2** com 5k mensagens sintéticas e alturas variáveis.
2. Implementar `VirtualTimeline` com:
   - cache de alturas medidas (`ResizeObserver` via web-sys / `onresize`),
   - janela com overscan, spacer superior/inferior,
   - ancoragem por `message_id` (não por índice) para inserts no topo/fundo,
   - scroll para mensagem, `scroll_to_bottom` em novas mensagens.
3. Alternativa: `dioxus-virtual-scroll` (comunidade) se o spike falhar.

Sem isso, o dashboard de 3.917 linhas não tem substituto aceitável.

### Estado

- `Signal<AppState>` global via context; `Memo` para seletores derivados.
- Um `use_future` por assinatura de evento, com `Drop` cancelando.
- Nenhuma chamada bloqueante no render: `spawn`/`spawn_blocking` para rusqlite e image.
- Login: máquina de estados explícita (`Phone → Code → Password → Ready`), como hoje.

---

## 6. Persistência (plasma-store)

Manter **os mesmos arquivos** para não invalidar caches de usuários existentes:

- `app_data_dir/plasma-hub.sqlite3` — mesma base, com `user_version`/migrations.
- `app_data_dir/twitter-cookies.json` (ou o nome atual) — mesmos caminhos.
- `app_cache_dir/plasma-media/**` e `telegram-media-cache/**` — mesmas pastas.
- `drop-video-thumbnails/**` — idem.

Novas tabelas (migração incremental, sem recriar o que existe):

```sql
CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL, updated_at INTEGER NOT NULL);
CREATE TABLE download_jobs (...);           -- opcional: substitui o JSON de histórico
CREATE TABLE twitter_sessions (...);        -- opcional
```

`SettingsStore` substitui `appStorage` com API tipada:

```rust
pub enum SettingKey { SkipLogin, Palette, Density, TwitterCookies, CacheMaxBytes, ... }
```

Escrita com debounce (350 ms, como no `DownloadService` atual) e flush no `Drop`/close.

> Compatibilidade: `skip_login`, paleta e densidade precisam ser lidos do storage antigo
> no primeiro boot da versão Rust (WebView `localStorage`), senão o usuário perde
> preferências. Fazer um one-shot migration: ler `localStorage` no primeiro start e
> gravar no SQLite.

---

## 7. Mídia, cache e playback (plasma-media)

- **Servidor HTTP em loopback** (porta aleatória + token por sessão, bind `127.0.0.1`):
  já implementado em `media_http.rs`; será promovido a crate e usado para **toda** mídia
  (imagens, thumbnails, áudio, vídeo), com `Range`/206.
- Remover dependência de `asset:` e `plasma-media://`; a UI usa
  `http://127.0.0.1:{port}/telegram/{chat}/{message}?token=...`.
- Metadados laterais (`.json`) e arquivos parciais (`.part`) mantidos; limpeza de
  parciais obsoletos e eviction por LRU/tamanho continuam como estão.
- Thumbnails: crate `image` (já usado) + cache em disco; geração de pôster de vídeo via
  ffmpeg (desktop) e `MediaMetadataRetriever` via JNI (Android) — hoje
  `generate_video_thumbnail` já cobre parte disso; validar parity no Android.
- Playback: `<video controls>` apontando para o loopback; mp4 faststart já é garantido por
  `existing_faststart_playback_file`.
- Android: permitir cleartext apenas para loopback (network security config) e manter a
  regra atual de esconder formatos adaptativos sem áudio no YouTube.

---

## 8. Camada de plataforma (plasma-platform)

| Capacidade | Desktop | Android |
|---|---|---|
| Paths (data/cache/downloads) | `dirs`/`directories` | `Context.filesDir`/`cacheDir` via JNI |
| File picker | `rfd` | SAF (`ACTION_OPEN_DOCUMENT`/`CREATE_DOCUMENT`) via JNI |
| Abrir/revelar arquivo | crate `opener` | `Intent.ACTION_VIEW` / FileProvider |
| Clipboard | `arboard` | `ClipboardManager` via JNI |
| Drag & drop | eventos do `tao`/`wry` | não aplicável |
| Share intent (URL) | deep link/CLI | `intent-filter` + JNI → `AppEvent::SharedUrl` |
| Notificações | `notify-rust` (opcional) | `NotificationManager` (opcional) |
| IME/texto | WebView | WebView |
| Tema/scale do SO | `tao` | config change → sinais |

O compartilhamento de URL no Android (hoje `sharedDownloadIntent.ts` + evento Tauri) passa
a ser um `intent-filter` no `AndroidManifest.xml` gerado pelo `dx` e um handler JNI que
publica `AppEvent::SharedUrl`.

---

## 9. Fases de execução

Cada fase termina com um gate verificável. O app Tauri continua compilando até a Fase 8.

| Fase | Entrega | Semanas | Gate |
|---|---|---:|---|
| **0. Bootstrap** | Workspace, crates vazios, mover Rust atual para crates com adaptadores mínimos, CI (`fmt`, `clippy`, `test`, `check` desktop+android) | 1–2 | `cargo test --workspace` verde; app Tauri ainda builda |
| **1. Core/store/platform** | `Error`, `AppPaths`, `SettingsStore` (com migração do localStorage), `Db`/migrations, `EventBus`, `tracing`, `platform` desktop | 1–2 | Testes de store/migração; paridade de paths |
| **2. Shell Dioxus** | Janela, abas, tema (6×3), design system, login/skip, command palette, toasts, CSS portado, **spike de virtualização** | 2–3 | App Rust abre, navega e loga (sem Telegram real) |
| **3. Telegram leitura** | Auth FSM, chat list, tópicos, timeline virtualizada, avatares, cache SQLite, busca de mensagens | 2–3 | Paridade de leitura com o app atual em conta real |
| **4. Telegram escrita/mídia** | Composer, envio, forward, shared media, previews/lightbox/player, cache de mídia, downloads, mass download, duplicados | 3–4 | Paridade funcional do dashboard |
| **5. Downloads + Twitter** | `DownloadManager` (fila, retry, cancel, batch, histórico), tela Downloads, Twitter library/cookies | 2–3 | Paridade de downloads e Twitter |
| **6. Plataformas web** | Instagram/Reddit em Rust; unificar YouTube/TikTok; formatos, headers, cookies, progresso | 2–3 | Download de URL nas 6 plataformas |
| **7. Android parity** | JNI, SAF, share intent, cleartext loopback, thumbnails nativos, permissões, assinatura, alinhamento 16 KB | 2–4 | APK `aarch64` com paridade do README |
| **8. Cutover** | Remover `src/`, `src-tauri/`, Vite/npm, scripts antigos; empacotamento, docs, CI final | 1 | Build limpo sem Node; smoke completo |

**Total:** 16–24 semanas (+ contingência).

### Fase 0 — concluída (2026-10-20)

1. `Cargo.toml` na raiz com workspace (`crates/*`, `apps/plasma`, `src-tauri`).
2. Extração concluída:
   - `plasma-core` (`AppEnv`, `AppPaths`, `EventEmitter`, `Error`);
   - `plasma-platform`, `plasma-store`, `plasma-download`, `plasma-media`,
     `plasma-duplicates`;
   - `plasma-telegram` (TDLib + servidor HTTP loopback);
   - `plasma-twitter`, `plasma-tiktok`, `plasma-youtube`;
   - placeholders `plasma-instagram`, `plasma-reddit`, `plasma-ui`, `apps/plasma`,
     `plasma-testkit`.
3. `src-tauri/src/services/` removido; `src-tauri/src/commands/*` agora são wrappers finos
   e `src-tauri/src/env.rs` faz a ponte `AppEnv`/eventos Tauri.
4. Os 62 comandos expostos ao webview foram preservados com os mesmos nomes e payloads.
5. Validação: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -D warnings`,
   `cargo test --workspace`, `cargo check -p plasma-hub` e `cargo check --target
   aarch64-linux-android` dos crates nativos.
6. Nenhuma mudança visível para o usuário; o frontend React e o schema SQLite
   (`plasma-hub.sqlite3`), cookies e pastas de mídia continuam iguais.

### Fase 1 — concluída (2026-10-20)

1. `plasma-store::db` centraliza a abertura do SQLite, pragmas (`WAL`, `foreign_keys`,
   `busy_timeout`) e migrações por `PRAGMA user_version`:
   - schema 1: tabelas existentes (`telegram_message_pages`, `telegram_messages`,
     `download_history`, `topic_download_state`), com `IF NOT EXISTS` para bancos antigos;
   - schema 2: tabela `settings (key, value, updated_at)`.
2. `SettingsStore` com API tipada (`SettingKey`), booleanos, JSON, snapshot e
   `import_legacy` idempotente para a migração one-shot do `localStorage`.
   As chaves usam exatamente os nomes atuais (`skip_login`, `plasma_appearance_palette`,
   `plasma_appearance_density`, etc.).
3. `Db`/`SettingsStore` são gerenciados no setup do Tauri; comandos `settings_get`,
   `settings_set`, `settings_remove`, `settings_snapshot` e `settings_import_legacy`
   ficam disponíveis para a ponte React durante a migração.
4. `plasma-core::EventBus` (`tokio::sync::broadcast`) e `AppEvent` definem o canal que a
   UI Rust vai consumir; o `EventEmitter`/eventos Tauri atuais continuam intactos.
5. Testes: criação do schema, upgrade de banco v0 sem perder linhas, idempotência,
   rejeição de schema futuro, round-trip de settings, snapshot ordenado e import legado.
6. Compatibilidade preservada: mesmo arquivo `plasma-hub.sqlite3`, mesmas tabelas e
   mesmos nomes de chave. Nenhum caminho de dados do usuário mudou.

### Fase 2 — shell implementado (2026-10-20)

1. `apps/plasma` (workspace aninhado) resolve os mesmos diretórios do Tauri
   (`<data>/com.camacho.plasmahub`, cache e downloads; `PLASMA_HOME` apenas para
   desenvolvimento), abre o SQLite e lança o Dioxus com janela 1280×820.
2. `plasma-ui` entrega o shell navegável: rail com Telegram/Downloads/Twitter/Ajustes,
   login de demonstração com “pular login”, seis paletas, três densidades, command palette,
   toasts, placeholders das abas e uma tela de diagnóstico com o caminho do banco/schema.
3. Preferências são lidas e gravadas no `SettingsStore` (`plasma_appearance_palette`,
   `plasma_appearance_density`, `skip_login`). O shell também consegue importar as chaves
   globais do `localStorage` da própria WebView.
4. Spike de virtualização: 5.000 mensagens sintéticas com alturas variáveis, prefix sums,
   busca binária da janela visível, overscan e apenas ~20–40 linhas montadas no DOM.
   Testes cobrem layout, limites do range e mensagens longas.
5. Migração real do frontend antigo: `src/shared/storage/settingsMigration.ts` roda no boot
   do React/Tauri e espelha as chaves globais no SQLite com `overwrite: true` (o React é a
   autoridade até o cutover). O shell Rust usa import sem sobrescrever.
6. Gate visual: `cargo check`, `clippy`, `cargo test` e a abertura da janela + criação do
   banco foram validados. Neste ambiente a captura da WebView fica em branco também para o
   `MiniBrowser` do WebKitGTK 2.52 (problema do runtime, não do shell), então a conferência
   visual deve ser feita em um desktop/runner com WebKitGTK compatível antes de fechar a fase.

### Fase 3 — Telegram leitura implementado (2026-10-20)

1. `plasma-ui::state::TelegramHandle` embrulha `plasma-telegram` e despacha cada chamada
   para um runtime Tokio dedicado, sem bloquear o executor do Dioxus. O shell abre o mesmo
   TDLib/SQLite do app Tauri; `PLASMA_HOME` continua sendo apenas override de desenvolvimento.
2. `apps/plasma/build.rs` embute `VITE_API_ID`/`VITE_API_HASH` (ou
   `PLASMA_TELEGRAM_API_*`) sem versionar segredos. O binário também recebe rpath para o
   `libtdjson` baixado pelo `tdlib-rs`, permitindo `cargo run` direto.
3. Login real: telefone, código, senha de duas etapas, estados de erro, reconexão e
   “continuar sem conta” como prévia. A senha e o código nunca são persistidos.
4. Chat list real com filtro local, atualização manual e por evento, tópicos de fórum,
   avatares do chat selecionado (data URL), timeline virtualizada de altura variável,
   carregamento de mensagens antigas e busca de mensagens no chat/tópico.
5. Cache SQLite compatível com o React: chaves `chatId` e `chatId::forum:topicId`; leitura
   imediata do cache, merge por ID, ordenação por data, gravação do histórico mesclado e
   marcador persistente de mensagens apagadas em todas as páginas do chat.
6. EventBus: o emissor do shell traduz `tdlib-auth-state`, `tdlib-new-message` e
   `tdlib-messages-deleted` para `AppEvent`, mantendo lista/timeline reativas sem polling.
   O servidor HTTP loopback de mídia também é iniciado no shell Rust.
7. Diagnóstico headless: `PLASMA_HOME=/tmp/... cargo run -- --telegram-smoke` inicializa o
   TDLib e imprime estado/contagem de chats sem abrir a WebView. Validado até
   `wait_phone_number` com as credenciais do `.env`.
8. Gate de leitura real: a sessão autenticada e a conferência visual continuam pendentes
   neste ambiente (WebKitGTK 2.52 pinta a janela em branco até no `MiniBrowser`). A lógica
   de backend foi exercitada pelo smoke; falta validar chats/tópicos/mídia em conta real.

### Fase 4 — Telegram escrita/mídia implementado (2026-10-20)

1. `TelegramHandle` expõe leitura de chat, capabilities, envio de texto, envio de mídia,
   forward, shared media, thumbnail, preparação de playback e salvamento de mídia. As
   chamadas reutilizam as funções TDLib já usadas pelo Tauri e continuam no runtime Tokio
   dedicado; a UI só recebe DTOs.
2. Composer real com resposta (`reply_to_id`), tópico atual, `Enter` para enviar,
   `Shift+Enter` para quebrar linha, estado de envio/erro, bloqueio por capabilities e
   atualização otimista da timeline e do preview da conversa. O texto e o cache SQLite são
   gravados com a mesma chave `chatId`/`chatId::forum:topicId` usada pelo React.
3. Anexos usam o seletor nativo `rfd` no desktop e enviam pelo caminho local via
   `tdlib_send_media`, com legenda opcional e o mesmo estado de resposta/tópico. No Android
   o seletor nativo fica para a Fase 7.
4. Forward: qualquer mensagem da timeline abre um seletor de conversas e chama
   `tdlib_forward_message`; o resultado é confirmado por toast e a lista é atualizada pelo
   EventBus.
5. Mídia: thumbnails sob demanda na timeline, painel de mídia compartilhada com grid,
   lightbox para foto/vídeo/arquivo, player via servidor HTTP loopback com token, abertura
   de anexos sem prévia e salvamento em disco. O botão “Mídia” abre o painel do chat atual.
6. Mensagens apagadas continuam marcadas no cache e na timeline; a gravação do marcador
   acontece antes do reload disparado pelo evento, evitando que a página de rede sobrescreva
   o estado local.
7. CSS novo cobre composer, respostas, thumbnails, ações de mensagem, painel de mídia,
   lightbox, forward e estados de erro/carregamento.
8. Testes: store cobre marcação de apagadas em chaves de chat e tópico; a timeline cobre
   altura de thumbnails e colapso de mensagens apagadas; helpers de formatação de mídia têm
   teste unitário. Validação executada: fmt, clippy, testes do workspace e do app aninhado,
   `cargo check -p plasma-hub`, `npm run build` e `npm test`.
9. Limitações desta fase: estatísticas/limpeza do cache de mídia e busca de duplicados
   ainda não têm tela no shell Rust; playback e seletor de arquivos no Android ficam para a
   Fase 7. O backend de escrita/mídia é o mesmo já usado pelo app Tauri.
10. Download em massa (2026-10-21): o botão “Download em massa” no cabeçalho do chat abre
   um painel com pasta de destino, seleção de tópico, divisão por usuário/álbum (separador
   ou comentário), progresso por arquivo e botão de parada. O shell consome os eventos
   `tdlib-download-progress` via `AppEvent::TelegramDownloadProgress` e chama
   `tdlib_start_mass_download`/`tdlib_stop_download` pelos mesmos serviços do backend.
11. Gate visual desktop (2026-10-21): o cache WebKitGTK em `$XDG_CACHE_HOME/<app>` pode
   ficar parcial após uma execução interrompida e deixar o WebProcess sem pintar a página.
   O composition root agora usa `AppPaths::cache_dir()/webview`, limpa esse diretório antes
   de criar o WebView e define `XDG_CACHE_HOME` para ele. Isso também isola o cache do shell
   dos demais apps WebKitGTK.
12. Menus de contexto e refino visual (2026-10-21): `plasma-ui::context` define os alvos
   (`ContextTarget`) e as ações do menu de botão direito; `plasma-ui::views::context_menu`
   renderiza o menu flutuante com ícones, atalhos, separadores, navegação por teclado
   (`ArrowUp`/`ArrowDown`/`Escape`) e ajuste automático dentro da viewport. O menu é
   renderizado na raiz do shell (`UiState::context_menu`) e abre em conversas, mensagens
   da timeline e itens do histórico de downloads. Ações: abrir conversa, marcar como lida,
   download em massa, copiar ID, responder, encaminhar, copiar texto, abrir/salvar mídia,
   abrir arquivo, mostrar na pasta, copiar caminho/link e remover do histórico. O shell
   também ganhou um conjunto próprio de ícones SVG (`plasma-ui::icons`) e uma rodada de
   refino visual (bubble de mensagem, indicador de conversa ativa, tooltips no rail,
   `focus-visible`, estados de status, `prefers-reduced-motion`).
13. Controles e menus (2026-10-21): a barra de menu nativa do Dioxus (Window/Edit/Help)
   foi removida com `Config::with_menu(None)`; botões, campos, switches e selects seguiram os
   tokens do design system React (`--control-height`, `--field-focus-border`,
   `--field-focus-ring`). O shell ganhou um `Select` próprio (`plasma-ui::views::select`) com
   chevron e check do conjunto de ícones SVG, e o composer de conversas somente-leitura
   agora mostra apenas um rodapé compacto com o aviso, sem campo de texto nem botões.
14. Selection mark nos controles (2026-10-21): botões, switches e o `Select` agora usam
   `user-select: none`, evitando a marca de seleção de texto que aparecia ao clicar nos
   controles (o DOM do dropdown muda entre `mousedown` e `mouseup`). Campos de texto e
   `textarea` mantêm a seleção de texto.
15. Cache de mídia e busca de duplicados (2026-10-21): os Ajustes passaram a exibir o
    tamanho do cache nativo de mídia, o limite aplicado (com evicção do mais antigo) e a
    limpeza do cache; a timeline ganhou um modo de seleção de mídias e o diálogo
    "Procurar arquivo no sistema", que usa `plasma-duplicates` (busca exata e varredura
    profunda) com progresso via `AppEvent::DuplicateSearchProgress` e ação de revelar o
    arquivo encontrado.
16. Foco neutro (2026-10-21): campos de texto e selects não usam mais anel colorido no
    foco (apenas a borda muda, em tom neutro) e o `::selection` segue a paleta, evitando
    o aspecto de "marca texto" apontado no visual.
17. Avatar das conversas (2026-10-21): a lista de chats passou a mostrar a foto real de
   cada grupo/conversa. `plasma-telegram` expõe `telegram_avatar_dir`, `cached_chat_avatar`
   e a rota `/avatar/{chat_id}` do servidor HTTP local; o download do avatar é feito sob
   demanda por linha (`plasma-ui::views::chat_list::ChatAvatar`) e fica em cache em
   `cache/plasma-media/telegram-avatars`, então a segunda visita não fala com o TDLib.
   A imagem é servida pelo loopback local (com token) em vez de base64 no DOM. Conversas
   sem foto mantêm a letra colorida como fallback.
18. Álbuns (2026-10-22): a timeline agrupa mensagens consecutivas com o mesmo `grouped_id`
   em um único item (`TimelineItem::Album`), como no dashboard React: grade de miniaturas,
   rótulo “álbum · N” e seleção por célula. O menu de contexto do álbum traz abrir mídia,
   salvar esta mídia, salvar o álbum inteiro, copiar legenda e encaminhar; o salvamento usa
   o cache de mídia (`ensure_media_cached`) e a pasta de downloads configurada.
19. Estabilidade da timeline (2026-10-22): o refresh em segundo plano (novas mensagens,
   edições, exclusões, botão “Atualizar conversas”) não desmonta mais a timeline.
   `UiState::messages_key` identifica a conversa exibida e `load_conversation` mantém a
   página visível (`keep_visible`) em vez de limpar as mensagens e mostrar “Carregando”; a
   lista de chats só exibe o placeholder na primeira carga. O scroll passou a ser
   intencional (`ScrollIntent`): abre no fim ao trocar de conversa, acompanha novas
   mensagens apenas quando o leitor já está no fim (distância < 120 px) e preserva a
   posição visual quando histórico antigo é pré-inserido (deslocamento pelo delta de
   altura). `MessageTimeline` recebe o `Signal` das mensagens e guarda os limites
   anteriores em um `CopyValue`, sem re-disparar o efeito a cada render.
20. Tópicos ocultos e listagem (2026-10-22): os chips de tópico dos grupos de fórum
   passaram a abrir um menu de contexto com “Ocultar tópico” (ou “Mostrar tópico”) e
   “Exibir listagem de tópicos”. A visibilidade é por conversa, persistida em
   `plasma_hidden_topics_<chat_id>` (JSON no `SettingsStore`, mesma chave usada pelo
   React), e o chip do tópico aberto continua visível para não esconder a conversa ativa.
   A listagem abre o diálogo `TopicBrowser` (`plasma-ui::views::topics`) com pesquisa,
   alternância “Exibir tópicos ocultos (N)”, linha “Todos os tópicos”, seleção do tópico,
   status (não lidas/fechado) e o mesmo menu de contexto por linha. O visual segue o shell
   (modal card, tokens, ícones SVG), não o painel React.
21. Select acima de modais (2026-10-22): a lista de opções do `Select` deixou de ser
   renderizada dentro do campo. `open_select` mede o trigger e publica a lista em
   `UiState::select_portal`; o shell renderiza `SelectMenu` na raiz com coordenadas de
   viewport, então `overflow: hidden` e `backdrop-filter` de modais/painéis não recortam
   mais o dropdown. A lista vira para cima quando não há espaço abaixo e o clique fora
   fecha o menu.
22. Desempenho da timeline (2026-10-22): `set_if_changed` passou a ser usado em chats,
    tópicos, mensagens, `has_more`, erros e carregamento, para que refreshes em segundo
    plano que devolvem os mesmos dados não notifiquem nem re-renderizem o shell. As
    thumbnails ficaram em um cache em memória (`(chat_id, message_id)` -> data URL, limite
    de 512) em `UiState::thumbnails`, então linhas que saem e voltam à janela virtualizada
    aparecem de imediato em vez de piscar o placeholder e baixar de novo. Um
    `ResizeObserver` no container mantém a última mensagem visível quando o viewport muda
    (composer, banner de resposta, redimensionamento) e o leitor já estava no fim.
23. Histórico automático (2026-10-22): a timeline deixou de depender só do botão
    “Carregar mensagens antigas”. Ao rolar até os 220 px iniciais, `MessageTimeline`
    chama `on_reach_start`, que pede a página anterior com o mesmo `load_older` (guardado
    por `loading_older` e `messages_has_more`), como o `startReached` do React. O botão
    continua visível como alternativa quando a lista é menor que o viewport.

### Fase 5 — downloads e Twitter implementados (2026-10-20)

1. `plasma-ui::downloads` introduz `DownloadRecord` e `DownloadHandle`. O record usa
   `camelCase`, preserva campos desconhecidos do histórico React via `#[serde(flatten)]`
   e reutiliza a tabela `download_history` do SQLite. Registros que ficaram
   `downloading` ao fechar o app viram `failed` com mensagem de interrupção, igual ao
   `DownloadService` React.
2. O shell restaura o histórico no boot, mescla com downloads ativos e persiste a fila em
   cada início/terminal. As gravações são serializadas por um `tokio::sync::Mutex` para
   evitar snapshots fora de ordem. O `EventBus` recebe progresso, conclusão e erro dos
   downloads nativos e do Twitter; `DownloadProgress` aceita percentual opcional para
   produtores que não informam bytes.
3. Tela Downloads: formulário HTTPS com nome opcional, pasta de destino, fila em memória,
   barra de progresso, cancelamento, retry (com retomada via arquivo parcial), remoção de
   itens e limpeza de finalizados. O histórico mostra plataforma, tamanho, origem, destino
   e erros. O limite persistido continua sendo 300 itens, como no React.
4. `plasma-ui::twitter` embrulha o analisador e o downloader nativos no runtime Tokio. A
   tela X/Twitter detecta automaticamente post versus perfil (URL, `@handle` ou handle),
   lista variantes de vídeo/áudio com tamanho e marca a melhor, baixa uma variante escolhida
   ou todas as mídias de um perfil, mostra avatar/thumbnail e mantém os cookies opcionais
   no mesmo arquivo usado pelo app Tauri (`load_twitter_cookies`/`save_twitter_cookies`).
5. Downloads do Twitter entram na mesma fila/histórico com `platform: "twitter"`,
   `sourcePageUrl`, lote e progresso. O perfil usa a pasta `@usuario`, igual ao backend
   nativo. Retry genérico fica desabilitado para itens do Twitter porque a análise precisa
   de cookies/contexto; o usuário reanalisa na aba X/Twitter.
6. CSS novo cobre a tela de downloads, formulário, progresso, estados, cookies, resultados
   de post, lista de formatos e grid de mídias do perfil, com ajuste responsivo.
7. Testes novos cobrem sanitização de nomes, derivação de nome pela URL, preservação de
   campos extras do histórico, extensões de mídia, slug de status e detecção de perfil.
   Validação: fmt, clippy `-D warnings`, testes do workspace e do app aninhado,
   `cargo check -p plasma-hub`, `npm run build` e `npm test`.
8. Limitações: a fila não impõe limite de downloads concorrentes; o formulário genérico só
   aceita URLs HTTPS diretas. Extração de YouTube/TikTok/Instagram/Reddit e mass download
   de Telegram entram na Fase 6. A validação visual e os downloads reais de Twitter com
   conta/cookies continuam pendentes neste ambiente (WebKitGTK em branco); os endpoints
   nativos são os mesmos usados pelo app Tauri.

### Fase 6 — extratores nativos das plataformas implementados (2026-10-20)

1. Novo crate de domínio `plasma-extract` unifica `MediaInfo`, `MediaFormats`,
   `FormatOption` e `Platform` para as seis opções do antigo downloader React:
   X/Twitter, YouTube, TikTok, Instagram, Reddit e web. A detecção normaliza
   `x.com`/`twitter.com`, variantes de YouTube/`youtu.be`, subdomínios de TikTok,
   Instagram, Reddit/`redd.it` e cai em `web` para links diretos.
2. YouTube é analisado pela própria página (`ytInitialPlayerResponse`), com parser de
   objeto JSON balanceado (não por busca de `;</script>`), formatos muxed/adaptive,
   `itag` numérico como ID de download, tamanho por `filesize`/`contentLength`, duração,
   views e thumbnail. No Android, formatos só-vídeo são omitidos; no desktop são
   exibidos com o marcador “sem áudio” e o download combina `+bestaudio` quando o
   usuário escolhe um deles.
3. TikTok reutiliza o analisador nativo (yt-dlp no desktop) e o fallback TikWM, agora
   público e compilado também no Android. Os DTOs de TikTok foram expostos e derivam
   `Clone`/`PartialEq` para conversão e props Dioxus. O download no Android usa a URL
   direta do TikWM com o baixador HTTPS compartilhado; no desktop continua no downloader
   nativo.
4. Instagram foi portado para Rust com GraphQL (`shortcode_media`, carrossel, vídeo,
   imagem, autor, duração e métricas) e fallback pelo HTML do embed. O embed usa
   user-agent móvel e decodifica JSON-em-JSON (URLs `https:\\/\\/...`, `\\uXXXX`). Reddit
   usa o JSON público de comentários, vídeo `DASH`, áudio derivado, imagem/link e
   thumbnail. Ambos baixam pela `plasma_download` com `Referer`/user-agent apropriados e
   entram no mesmo histórico.
5. X/Twitter foi convertido para o modelo unificado sem duplicar a lógica de análise.
   O download de Twitter e dos demais formatos é despachado por `DownloadSpec`, que
   escolhe o downloader nativo correto ou o download HTTPS direto.
6. `ExtractorHandle` no shell usa o runtime Tokio compartilhado, carrega os cookies
   salvos do Twitter quando necessário e expõe `analyze`/`download` para a UI. A tela
   Downloads ganhou o formulário “Analisar link”, cartão de resultado com thumbnail,
   autor, métricas, formatos e botões de download, mantendo o formulário de URL HTTPS
   direta. O histórico agora persiste `thumbnailUrl`, `sourceMode` e `sourceFormatId`.
7. O emissor do shell traduz `youtube-download-progress/done/error` e
   `tiktok-download-progress/done/error` para os mesmos eventos de download usados pelo
   Twitter e pelos arquivos diretos.
8. Testes cobrem detecção de plataforma, IDs de YouTube/Instagram/Reddit, parser de JSON
   embutido, decodificação de URLs escapadas, sanitização/formatação e extensões de mídia.
   Validação de rede executada para análise real de YouTube (título, thumbnail e formatos)
   e de um reel público do Instagram (vídeo, thumbnail, autor e legendas). Reddit
   respondeu HTTP 403 neste IP e TikTok não foi exercitado; downloads completos continuam
   dependentes de rede/conta.
9. Limitações: Instagram pode exigir login/GraphQL atualizado e não há tela de cookies
   dedicada; Reddit pode responder 403 em IPs bloqueados; YouTube não trata vídeos com
   idade/região restrita; retry de downloads de plataforma é feito reanalisando o link
   (não pelo botão genérico); cache de mídia e duplicados continuam pendentes. O código
   Android foi validado com `cargo check` e
   `cargo clippy --target aarch64-linux-android` usando NDK 27.1; execução de UI, SAF,
   players e downloads Android continuam pendentes para a Fase 7.

### Fase 7 — paridade Android implementada (2026-10-21)

1. `plasma-platform::android` usa `ndk-context` + JNI 0.21 para resolver
   `filesDir`/`cacheDir`/`getExternalFilesDir("Downloads")` sem depender de `tauri` ou
   `dioxus`. O shell usa esses caminhos no Android e mantém o mesmo `plasma-hub.sqlite3`,
   cookies e diretórios de mídia do app Tauri; `PLASMA_HOME` continua sendo override de
   desenvolvimento.
2. O seletor de mídia do Telegram no Android usa SAF (`ACTION_OPEN_DOCUMENT`). O
   `MainActivity` customizado copia o `content://` escolhido para o cache do app e devolve
   o caminho local ao Rust por JNI, resolvendo o future assíncrono da UI. Desktop continua
   usando `rfd`.
3. Share intents: o manifesto registra `SEND`/`SEND_MULTIPLE` (`text/*`) e `VIEW` para
   YouTube, Instagram, TikTok, X/Twitter e Reddit. O Activity lê o intent inicial e
   `onNewIntent`, e o Rust publica `AppEvent::SharedUrl`; a UI alterna para Downloads,
   preenche a URL e dispara a análise automaticamente. URLs recebidas antes do mount ficam
   pendentes em `AppServices::initial_shared_url`.
4. Downloads concluídos ganharam “Abrir” e “Mostrar” usando `plasma_platform::open_path` /
   `reveal_path`. No Android, arquivos em `getExternalFilesDir` são indexados pelo
   `MediaScannerConnection` e abertos por `ACTION_VIEW`; “Mostrar” cai para abrir o próprio
   arquivo porque o Android não tem um reveal equivalente.
5. O servidor HTTP loopback deixou de ser desktop-only: Android usa
   `http://127.0.0.1:{porta}` com token de sessão e `Range`/206 para todo o playback e
   thumbnails. O `AndroidManifest.xml` usa `network_security_config` que permite cleartext
   **somente** para `127.0.0.1`, com `usesCleartextTraffic=false`.
6. Thumbnails de vídeo no Android usam `MediaMetadataRetriever` via JNI, com fallback de
   frames em 1 s/0,25 s/0 s e JPEG em cache. `plasma-media` mantém `ffmpeg` no desktop e a
   mesma API para `plasma-telegram` e downloads.
7. `apps/plasma/Dioxus.toml` aponta para o manifesto e o `MainActivity.kt` customizados.
   O manifesto inclui `extractNativeLibs`, share filters, tema e configuração de rede; o
   ProGuard mantém o Activity e os métodos nativos no release.
8. Empacotamento: `scripts/build-dioxus-android.sh` (também `npm run build:ui:android`)
   roda `dx build --android --target aarch64-linux-android`, copia o `libtdjson.so` do
   `tdlib-rs` para o `jniLibs/arm64-v8a` gerado e roda o Gradle. Isso corrige a lacuna do
   `dx`, que liga o `libmain.so` ao TDLib mas não empacota a biblioteca compartilhada.
   O APK debug `aarch64` foi gerado com Dioxus CLI 0.7.10, NDK 27.1, compile/target SDK 36
   e validado com `aapt2` (manifesto/rede/share filters); `libmain.so` e `libtdjson.so`
   estão alinhados em 16 KB (`p_align=0x4000`) e exportam os callbacks JNI.
9. Assinatura: o keystore existente em `src-tauri/gen/android` é local e não é versionado.
   O gate usa APK debug assinado automaticamente; releases devem usar o keystore local
   (Android Studio/Gradle) ou secrets de CI, nunca `Dioxus.toml` com senha. Updater próprio
   continua em aberto para a Fase 8.
10. Limitações desta fase: não há device/emulator neste ambiente, então SAF, share,
    player, thumbnail e abertura de arquivo foram validados por compilação, inspeção do
    APK/`nm`/`aapt2`, não por execução. A abertura no Android depende do MediaScanner para
    arquivos externos; arquivos internos (ex.: mídia do Telegram) ainda não têm
    `FileProvider`. Clipboard, notificações e fullscreen de vídeo permanecem sem validação
    de device. A janela desktop continua sem render visual neste ambiente (WebKitGTK).

---

## 10. Riscos e mitigações

| Risco | Prob. | Impacto | Mitigação |
|---|---|---|---|
| Virtualização de timeline com altura variável | Alta | Alto | Spike na Fase 2; ancoragem por ID; fallback `dioxus-virtual-scroll`; limite de mensagens em memória |
| Vídeo no WebView Android (codecs, cleartext, fullscreen) | Média-alta | Alto | Testar cedo em device real; network security config só para loopback; manter restrições atuais de formatos |
| Dioxus 0.7 → 0.8 (APIs mudam) | Média | Médio | Pin `=0.7.10`; upgrade só após cutover; isolar APIs de janela em `apps/plasma` |
| WebKitGTK recente + `wry 0.53` (Dioxus 0.7) | Média | Alto | Validar o renderer cedo em desktop/Android; se o WebView não pintar, testar Dioxus 0.8 alpha ou voltar ao plano B sem perder os crates de domínio |
| UI travando por chamadas bloqueantes (rusqlite/image/TDLib) | Média | Alto | `spawn_blocking`, lint/clippy, timeouts; medir frame time com 10k mensagens |
| Volume de eventos TDLib (novas mensagens, progresso) | Média | Médio | Coalescing, canais com backpressure, processamento em lote |
| Perda de dados do usuário (localStorage/SQLite) | Baixa | Alto | Migração one-shot + backup `.bak` antes de migrar; testes com DB real |
| Paridade Android (share intent, SAF, thumbnails) | Média | Alto | Fase 7 dedicada; checklist do README atual vira critério de aceite |
| Empacotamento/assinatura/atualização sem Tauri | Média | Médio | `dx bundle` + Gradle; decidir updater no início da Fase 7 |
| Esforço subestimado | Alta | Alto | Fases verticais, gates; nada de "big bang"; Tauri fica funcional até o fim |

### Critério de abortar/voltar ao plano B

Se até o fim da Fase 3 o spike de virtualização e o playback no Android não estiverem
satisfatórios, congelar a UI Rust e adotar o plano B (Seção 11) sem perder o trabalho de
domínio já extraído — os crates são reaproveitados integralmente pelo Tauri.

---

## 11. Plano B (registro)

Manter Tauri 2 + React e mover para Rust tudo que ainda é lógica:

- extractors Instagram/Reddit, `DownloadService`, `MediaCacheService` restante,
  `TelegramService` (orquestração), stores;
- React fica só com view/estado de tela, chamando comandos tipados.

Isso entrega ~80% do benefício (uma linguagem no domínio, menos duplicação) com ~30–40%
do esforço e risco muito menor, mas **não** atende ao pedido literal de "app inteiro em
Rust".

---

## 12. Validação e CI

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --exclude plasma-hub
cargo check -p plasma-hub
cd apps/plasma && cargo fmt --all --check
cd apps/plasma && cargo clippy --all-targets -- -D warnings
cd apps/plasma && cargo test
cargo check -p plasma-telegram --target aarch64-linux-android
scripts/build-dioxus-android.sh debug   # Fase 7+; requer dx 0.7.10 e NDK
```

Testes que valem a pena desde a Fase 0:

- Conversores TDLib → DTO (funções puras; hoje sem testes).
- Extractors com fixtures (`yt_test_web.json`, `yt_test_tv.json`, `yt_test_ios.json`,
  `yt_initial.json`) reaproveitados como golden files.
- Máquina de estados de download (cancelado ≠ falho ≠ completo).
- Migração de settings e schema SQLite.
- Busca de duplicados (hash perceptual).

Smoke manual por fase (herdado do README): login/skip, navegação Telegram, envio,
download de mídia, aba Downloads, Twitter library, Settings/tema, share no Android.

---

## 13. Decisões pendentes (preciso do seu ok)

1. **Aprovar Dioxus 0.7.10 + renderer WebView** (vs. Slint/egui ou plano B).
2. **Aprovar o workspace de crates** e a regra "domínio não conhece UI".
3. **Cutover por fases** com Tauri funcional até a Fase 8 (não big bang).
4. **Manter compatibilidade** de SQLite, cookies e pastas de mídia (recomendado) ou
   começar limpo.
5. **Android:** manter apenas `arm64-v8a` e API mínima atual? Precisa de foreground
   service para downloads em background?
6. **YouTube desktop:** manter o sidecar `yt-dlp` (baixado em runtime) ou remover?
7. **Idioma da UI:** manter strings em PT hardcoded (como hoje) ou já preparar i18n?
8. **Atualizações/assinatura:** `dx bundle` + updater próprio ou distribuição manual?
9. **Nome/estrutura do binário:** `plasma` em `apps/plasma` ou manter `plasma-hub`.

Após o ok, começo pela Fase 0 (workspace + extração dos crates + CI), sem tocar na UI.
