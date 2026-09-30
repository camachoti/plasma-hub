# TODO UX

Backlog de melhorias de usabilidade para o Plasma Hub, priorizado por impacto percebido e risco técnico.

## Prioridade Alta

- [x] Padronizar estados principais por chat: carregando mensagens, carregando mídias, sem permissão para enviar, TDLib desconectado e reconectando.
- [x] Criar skeletons específicos para mensagem, avatar, mídia, lista de chats e lista de tópicos.
- [x] Melhorar feedback de mídia, separando claramente cache, download, processamento, salvamento, cancelamento e indisponibilidade.
- [x] Implementar busca dentro do chat com filtros rápidos por mídia, usuário, data, álbum e tópico.
- [x] Criar barra de ações fixa no modo seleção com baixar, encaminhar, copiar e cancelar seleção.

## Prioridade Média

- [x] Melhorar empty states para grupo sem tópicos, tópico vazio, chat sem mídia, login pendente e TDLib não autenticado.
- [x] Adicionar atalhos de teclado: `Esc`, `/`, `Ctrl+F`, `Enter` e `Shift+Enter`.
- [x] Persistir preferências por chat, incluindo ordem personalizada, último chat, tópico, filtros, painel lateral e posição aproximada de leitura.
- [x] Refinar pré-carregamento inteligente: priorizar thumbs visíveis, manter próximas em segundo plano e pausar atualizações em scroll rápido.
- [x] Adicionar toasts discretos para downloads, envio, pasta selecionada, arquivo anexado, mídia salva, cópia e encaminhamento.

## Prioridade Técnica Com Impacto UX

- [x] Criar fila visual única de downloads com progresso, histórico persistente, remoção, abrir pasta, retry e cancelamento, incluindo YouTube/Twitter nativos.
- [x] Exibir indicador discreto quando mídia vem do cache versus rede.
- [x] Criar banner superior para TDLib desconectado, reconectando ou atualizando.
- [x] Reduzir flashes de estado, especialmente “mídia indisponível” durante transições de carregamento.
- [x] Restaurar sessão de UI após reiniciar, incluindo chat, tópico, painel lateral e posição aproximada de leitura.
- [x] Revisar acessibilidade dos controles de mídia, mensagens e cabeçalho: rótulos para leitor de tela, estado expandido/pressionado e navegação por teclado.

## Sequência Recomendada

1. Persistência de ordem personalizada.
2. Pré-carregamento inteligente de mídias.
3. Redução de flashes de estado.
4. Revisão final de acessibilidade.
