import React from 'react';
import { LoadingIndicator, TextArea } from '../../design-system';
import { IconX } from "../../design-system/icons";
import type { Chat, ChatFullInfo, Message } from './TelegramDashboardTypes';
import { IconAttach, IconDownload, IconEmoji, IconSend } from './DashboardIcons';

interface SelectedFile {
  filePath: string;
  fileName: string;
}

interface SelectionActionBarProps {
  selectedMessageIds: number[];
  selectableMediaIds: number[];
  bulkDownloadActive: boolean;
  bulkProgress: {
    total: number;
    downloaded: number;
    currentFile: string;
    status: string;
  } | null;
  handleBulkDownload: (messageIds: number[]) => Promise<boolean>;
  handleCopySelected: (messageIds: number[]) => Promise<void>;
  handleForwardSelected: (messageIds: number[]) => Promise<void>;
  setIsSelectionMode: React.Dispatch<React.SetStateAction<boolean>>;
  setSelectedMessageIds: React.Dispatch<React.SetStateAction<number[]>>;
}

const SelectionActionBarComponent: React.FC<SelectionActionBarProps> = ({
  selectedMessageIds,
  selectableMediaIds,
  bulkDownloadActive,
  bulkProgress,
  handleBulkDownload,
  handleCopySelected,
  handleForwardSelected,
  setIsSelectionMode,
  setSelectedMessageIds,
}) => {
  const selectedCount = selectedMessageIds.length;
  const selectableCount = selectableMediaIds.length;
  const allLoadedSelected = selectableCount > 0 && selectedCount >= selectableCount;
  const progressPercent = bulkProgress?.total
    ? Math.min(100, Math.round((bulkProgress.downloaded / bulkProgress.total) * 100))
    : 0;

  const handleDownloadSelected = async () => {
    const started = await handleBulkDownload(selectedMessageIds);
    if (started) {
      setIsSelectionMode(false);
      setSelectedMessageIds([]);
    }
  };

  return (
    <div className="selection-action-bar-wrap">
      <div className="selection-action-bar">
        <div className="selection-summary">
          <span className="selection-summary-main">
            {selectedCount} {selectedCount === 1 ? 'mídia selecionada' : 'mídias selecionadas'}
          </span>
          <span className="selection-summary-sub">
            {bulkDownloadActive && bulkProgress
              ? `${progressPercent}% baixado · ${bulkProgress.currentFile}`
              : `${selectableCount} ${selectableCount === 1 ? 'mídia carregada' : 'mídias carregadas'} no chat`}
          </span>
          {bulkDownloadActive && bulkProgress && (
            <div className="selection-progress" aria-label="Progresso do download selecionado">
              <div className="selection-progress-fill" style={{ width: `${progressPercent}%` }} />
            </div>
          )}
        </div>
        <div className="selection-actions">
          <div className="selection-secondary-actions">
            <button
              className="btn-selection-secondary"
              disabled={bulkDownloadActive || selectableCount === 0 || allLoadedSelected}
              onClick={() => setSelectedMessageIds(selectableMediaIds)}
            >
              Selecionar tudo
            </button>
            <button
              className="btn-selection-secondary"
              disabled={bulkDownloadActive || selectedCount === 0}
              onClick={() => setSelectedMessageIds([])}
            >
              Limpar
            </button>
          </div>
          <button
            className="btn-cancel-selection"
            disabled={bulkDownloadActive}
            onClick={() => {
              setIsSelectionMode(false);
              setSelectedMessageIds([]);
            }}
          >
            Sair
          </button>
          <button
            className="btn-selection-secondary"
            disabled={selectedCount === 0 || bulkDownloadActive}
            onClick={() => void handleCopySelected(selectedMessageIds)}
          >
            Copiar
          </button>
          <button
            className="btn-selection-secondary"
            disabled={selectedCount === 0 || bulkDownloadActive}
            onClick={() => void handleForwardSelected(selectedMessageIds)}
          >
            Encaminhar
          </button>
          <button
            className="btn-download-selected"
            disabled={selectedCount === 0 || bulkDownloadActive}
            onClick={handleDownloadSelected}
          >
            <IconDownload />
            <span>{bulkDownloadActive ? 'Baixando...' : `Baixar ${selectedCount || ''}`.trim()}</span>
          </button>
        </div>
      </div>
    </div>
  );
};

interface MessageComposerProps {
  inputText: string;
  isSending: boolean;
  replyTo: Message | null;
  selectedFile: SelectedFile | null;
  sendProgress: number | null;
  canSendMessages: boolean;
  canSendMedia: boolean;
  handleSelectFile: () => void;
  handleSend: () => void;
  setInputText: React.Dispatch<React.SetStateAction<string>>;
  setReplyTo: React.Dispatch<React.SetStateAction<Message | null>>;
  setSelectedFile: React.Dispatch<React.SetStateAction<SelectedFile | null>>;
}

const MessageComposerComponent: React.FC<MessageComposerProps> = ({
  inputText,
  isSending,
  replyTo,
  selectedFile,
  sendProgress,
  canSendMessages,
  canSendMedia,
  handleSelectFile,
  handleSend,
  setInputText,
  setReplyTo,
  setSelectedFile,
}) => (
  <div className="composer-wrap">
    <div className="composer">
      {replyTo && (
        <div className="reply-strip">
          <div className="reply-bar" />
          <span className="reply-from">Respondendo</span>
          <span className="reply-text">{replyTo.text ? replyTo.text.slice(0, 80) : 'Mídia'}</span>
          <button type="button" className="close icon-btn" onClick={() => setReplyTo(null)} aria-label="Cancelar resposta"><IconX size={18} stroke={2} /></button>
        </div>
      )}
      {selectedFile && (
        <div className="composer-file-chip">
          <span className="chip">
            <IconAttach /> {selectedFile.fileName}
            <button type="button" className="icon-btn" style={{ width: 18, height: 18 }} onClick={() => setSelectedFile(null)} aria-label="Remover arquivo anexado"><IconX size={16} stroke={2} /></button>
          </span>
        </div>
      )}
      {sendProgress !== null && (
        <div className="composer-progress">
          <div className="composer-progress-fill animated-stripes" style={{ width: `${sendProgress}%` }} />
        </div>
      )}
      <div className="composer-main">
        <button type="button" className="icon-btn" onClick={handleSelectFile} disabled={isSending || !canSendMedia} title={canSendMedia ? 'Anexar arquivo' : 'Envio de mídia indisponível'} aria-label={canSendMedia ? 'Anexar arquivo' : 'Envio de mídia indisponível'}>
          <IconAttach />
        </button>
        <TextArea
          appearance="inline"
          value={inputText}
          onChange={event => {
            if (canSendMessages) setInputText(event.target.value);
          }}
          onInput={event => {
            const textarea = event.currentTarget;
            textarea.style.height = 'auto';
            textarea.style.height = `${Math.min(textarea.scrollHeight, 160)}px`;
          }}
          onKeyDown={event => {
            if (event.key === 'Enter' && !event.shiftKey) {
              event.preventDefault();
              handleSend();
            }
          }}
          placeholder={canSendMessages ? 'Escreva uma mensagem...' : 'Envio de mensagens indisponível neste grupo'}
          rows={1}
          disabled={isSending || !canSendMessages}
        />
        <div className="composer-right-actions">
          <button type="button" className="icon-btn" title="Emoji" aria-label="Adicionar emoji">
            <IconEmoji />
          </button>
          <button
            type="button"
            className="composer-send"
            onClick={handleSend}
            disabled={isSending || ((!canSendMessages || !inputText.trim()) && (!canSendMedia || !selectedFile))}
          >
            {isSending ? <LoadingIndicator size="sm" /> : <IconSend />}
          </button>
        </div>
      </div>
    </div>
  </div>
);

interface JoinChannelBarProps {
  fullChatInfo: ChatFullInfo | null;
  selectedChat: Chat;
  onJoin: () => void;
}

const JoinChannelBarComponent: React.FC<JoinChannelBarProps> = ({
  fullChatInfo,
  selectedChat,
  onJoin,
}) => (
  <div className="join-channel-bar">
    <div className="join-channel-info">
      <h3>{selectedChat.title}</h3>
      {fullChatInfo?.participantsCount !== undefined && (
        <span>{fullChatInfo.participantsCount.toLocaleString()} participantes</span>
      )}
    </div>
    <button className="join-channel-btn" onClick={onJoin}>
      ENTRAR NO {selectedChat.isChannel ? 'CANAL' : 'GRUPO'}
    </button>
  </div>
);

export const SelectionActionBar = React.memo(SelectionActionBarComponent);
export const MessageComposer = React.memo(MessageComposerComponent);
export const JoinChannelBar = React.memo(JoinChannelBarComponent);
