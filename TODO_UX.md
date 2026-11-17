# TODO: UX Improvements

- [x] Adicionar toasts discretos para downloads, envio, pasta selecionada, arquivo anexado, mídia salva, cópia e encaminhamento.
- [x] Substituir ícone de exclamação no botão 'Downloads' por indicação visual sutil:
  - [x] Animacão suave (pulsação de opacidade no ícone) enquanto há downloads ativos
  - [x] Mudança de cor do botão (cor de destaque) quando há downloads em andamento
  - [x] Ambos (animacão + mudança de cor) — classe `.rail-item.busy` + `.icon` animado; falhas mantêm um ponto vermelho discreto (`.rail-dot`) sem exclamação
- [x] Progresso em tempo real do download em massa via `AppEvent::TelegramDownloadProgress` (o painel lê `mass_download_progress`)
- [x] Refatorar `apply_telegram_media_progress` para usar `Signal` diretamente em vez de `UiState` (wrapper sobre `apply_media_progress_to_items`, com testes unitários)
- [x] Ajustar layout da barra de status para melhorar a visibilidade do estado de download (pills com cor de estado + barra de progresso agregado em `DownloadsView`)
- [ ] Testar experiência de uso com usuários reais (se possível)

> **Nota**: O foco é manter uma interface limpa e intuitiva, evitando notificações intrusivas. A indicação visual deve ser perceptível mas não dominante.