# TODO UX

Backlog de melhorias de usabilidade para o Plasma Hub, priorizado por impacto percebido e risco técnico.

## Prioridade Alta

- [x] Padronizar estados principais por chat: carregando mensagens, carregando mídias, sem permissão para enviar, TDLib desconectado e reconectando.
- [x] Criar skeletons específicos para mensagem, avatar, mídia, lista de chats e lista de tópicos.
- [x] Melhorar feedback de mídia, separando claramente cache, download, processamento, salvamento, cancelamento e indisponibilidade.
- [x] Implementar busca dentro do chat com filtros rápidos por mídia, usuário, data, álbum e tópico.
- [x] Criar barra de ações fixa no modo seleção com baixar, encaminhar, copiar e cancelar seleção.

## Prioridade Média

- [ ] Melhorar empty states para grupo sem tópicos, tópico vazio, chat sem mídia, login pendente e TDLib não autenticado. Parcial: grupo sem tópicos e timeline/tópico vazio já foram refinados.
- [x] Adicionar atalhos de teclado: `Esc`, `/`, `Ctrl+F`, `Enter` e `Shift+Enter`.
- [ ] Persistir preferências por chat. Parcial: último chat, tópico, filtros e painel lateral já são restaurados; faltam ordem e posição do scroll.
- [ ] Refinar pré-carregamento inteligente: priorizar thumbs visíveis, depois próximas, depois álbuns, pausando em scroll rápido.
- [x] Adicionar toasts discretos para downloads, envio, pasta selecionada, arquivo anexado, mídia salva, cópia e encaminhamento.

## Prioridade Técnica Com Impacto UX

- [x] Criar fila visual única de downloads com progresso, histórico persistente, remoção, abrir pasta, retry e cancelamento, incluindo YouTube/Twitter nativos.
- [ ] Exibir indicador discreto quando mídia vem do cache versus rede.
- [x] Criar banner superior para TDLib desconectado, reconectando ou atualizando.
- [ ] Reduzir flashes de estado, especialmente “mídia indisponível” durante transições de carregamento.
- [ ] Restaurar sessão de UI após reiniciar. Parcial: chat, tópico e painel lateral são restaurados; falta posição aproximada do scroll.

## Sequência Recomendada

1. Empty states de chat sem mídia, login pendente e TDLib não autenticado.
2. Restauração de scroll e ordem.
3. Revisão final de acessibilidade.
