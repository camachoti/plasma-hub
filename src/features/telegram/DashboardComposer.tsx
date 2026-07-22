import React from 'react';
import type { Chat, ChatFullInfo, Message } from './TelegramDashboardTypes';
import { IconAttach, IconEmoji, IconSend } from './DashboardIcons';

interface SelectedFile {
  filePath: string;
  fileName: string;
}

interface SelectionActionBarProps {
  selectedMessageIds: number[];
  handleBulkDownload: (messageIds: number[]) => void;
  setIsSelectionMode: React.Dispatch<React.SetStateAction<boolean>>;
  setSelectedMessageIds: React.Dispatch<React.SetStateAction<number[]>>;
}

export const SelectionActionBar: React.FC<SelectionActionBarProps> = ({
  selectedMessageIds,
  handleBulkDownload,
  setIsSelectionMode,
  setSelectedMessageIds,
}) => (
  <div className="selection-action-bar-wrap">
    <div className="selection-action-bar">
      <span className="selection-count">
        {selectedMessageIds.length} {selectedMessageIds.length === 1 ? 'mídia selecionada' : 'mídias selecionadas'}
      </span>
      <div className="selection-actions">
        <button
          className="btn-cancel-selection"
          onClick={() => {
            setIsSelectionMode(false);
            setSelectedMessageIds([]);
          }}
        >
          Cancelar
        </button>
        <button
          className="btn-download-selected"
          disabled={selectedMessageIds.length === 0}
          onClick={() => {
            handleBulkDownload(selectedMessageIds);
            setIsSelectionMode(false);
            setSelectedMessageIds([]);
          }}
        >
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" style={{ marginRight: 6 }}>
            <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
            <polyline points="7 10 12 15 17 10" />
            <line x1="12" x2="12" y1="15" y2="3" />
          </svg>
          <span>Baixar Selecionadas</span>
        </button>
      </div>
    </div>
  </div>
);

interface MessageComposerProps {
  inputText: string;
  isSending: boolean;
  replyTo: Message | null;
  selectedFile: SelectedFile | null;
  sendProgress: number | null;
  handleSelectFile: () => void;
  handleSend: () => void;
  setInputText: React.Dispatch<React.SetStateAction<string>>;
  setReplyTo: React.Dispatch<React.SetStateAction<Message | null>>;
  setSelectedFile: React.Dispatch<React.SetStateAction<SelectedFile | null>>;
}

export const MessageComposer: React.FC<MessageComposerProps> = ({
  inputText,
  isSending,
  replyTo,
  selectedFile,
  sendProgress,
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
          <button type="button" className="close icon-btn" onClick={() => setReplyTo(null)}>✕</button>
        </div>
      )}
      {selectedFile && (
        <div className="composer-file-chip">
          <span className="chip">
            <IconAttach /> {selectedFile.fileName}
            <button type="button" className="icon-btn" style={{ width: 18, height: 18 }} onClick={() => setSelectedFile(null)}>✕</button>
          </span>
        </div>
      )}
      {sendProgress !== null && (
        <div className="composer-progress">
          <div className="composer-progress-fill animated-stripes" style={{ width: `${sendProgress}%` }} />
        </div>
      )}
      <div className="composer-main">
        <button type="button" className="icon-btn" onClick={handleSelectFile} disabled={isSending} title="Anexar arquivo">
          <IconAttach />
        </button>
        <textarea
          value={inputText}
          onChange={event => setInputText(event.target.value)}
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
          placeholder="Escreva uma mensagem..."
          rows={1}
          disabled={isSending}
        />
        <div className="composer-right-actions">
          <button type="button" className="icon-btn" title="Emoji">
            <IconEmoji />
          </button>
          <button
            type="button"
            className="composer-send"
            onClick={handleSend}
            disabled={(!inputText.trim() && !selectedFile) || isSending}
          >
            {isSending ? <span className="spinner small-spinner" /> : <IconSend />}
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

export const JoinChannelBar: React.FC<JoinChannelBarProps> = ({
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
