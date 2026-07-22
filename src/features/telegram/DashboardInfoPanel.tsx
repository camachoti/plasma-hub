import React from 'react';
import { ChatAvatar } from '../../components/ChatAvatar';
import { MessageMedia } from '../../components/MessageMedia';
import { hashColor } from './TelegramDashboardConstants';
import type { Chat, ChatFullInfo, ForumTopic, Message } from './TelegramDashboardTypes';

interface DashboardInfoPanelProps {
  density: string;
  forumTopics: ForumTopic[];
  fullChatInfo: ChatFullInfo | null;
  infoOpen: boolean;
  loadingFullInfo: boolean;
  loadingSharedMedia: boolean;
  messagesCount: number;
  palette: string;
  selectedChat: Chat | null;
  sharedMedia: Message[];
  getChatKind: (chat: Chat) => string;
  getDownloadMeta: (msg: Message, senderName?: string | null) => Record<string, unknown>;
  setInfoOpen: React.Dispatch<React.SetStateAction<boolean>>;
}

export const DashboardInfoPanel: React.FC<DashboardInfoPanelProps> = ({
  density,
  forumTopics,
  fullChatInfo,
  infoOpen,
  loadingFullInfo,
  loadingSharedMedia,
  messagesCount,
  palette,
  selectedChat,
  sharedMedia,
  getChatKind,
  getDownloadMeta,
  setInfoOpen,
}) => (
  <div className="info" data-collapsed={!infoOpen}>
    {selectedChat && (
      <>
        <div className="info-header">
          <span className="title">Informações</span>
          <button className="icon-btn" onClick={() => setInfoOpen(false)}>✕</button>
        </div>
        <div className="info-hero">
          <div className={`info-avatar color-${hashColor(selectedChat.id)}`}>
            <ChatAvatar chatId={selectedChat.id} title={selectedChat.title} />
          </div>
          <div className="info-name">{selectedChat.title}</div>
          <div className="info-sub">{getChatKind(selectedChat)}{selectedChat.hasTopics ? ' · fórum' : ''}</div>
        </div>

        {fullChatInfo?.about && (
          <div className="info-section">
            <h3>Bio / Descrição</h3>
            <div style={{ fontSize: 13, color: 'var(--text-1)', lineHeight: 1.5, whiteSpace: 'pre-wrap' }}>
              {fullChatInfo.about}
            </div>
          </div>
        )}

        <div className="info-section">
          <h3>Detalhes</h3>
          <div className="info-stats-grid">
            <div className="info-stat">
              <span className="info-stat-value">
                {loadingFullInfo ? '...' : fullChatInfo?.participantsCount?.toLocaleString() || '0'}
              </span>
              <span className="info-stat-label">Membros</span>
            </div>
            <div className="info-stat">
              <span className="info-stat-value">{messagesCount}</span>
              <span className="info-stat-label">Mensagens</span>
            </div>
          </div>

          <div style={{ marginTop: 16, display: 'flex', flexDirection: 'column', gap: 10 }}>
            <div className="info-field">
              <span className="info-field-label">ID</span>
              <span className="info-field-value">{selectedChat.id}</span>
            </div>
            {fullChatInfo?.username && (
              <div className="info-field">
                <span className="info-field-label">Username</span>
                <span className="info-field-value" style={{ color: 'var(--accent)' }}>@{fullChatInfo.username}</span>
              </div>
            )}
            {selectedChat.hasTopics && (
              <div className="info-field">
                <span className="info-field-label">Tópicos</span>
                <span className="info-field-value">{forumTopics.length}</span>
              </div>
            )}
          </div>
        </div>

        <div className="info-section">
          <h3>Mídia Compartilhada</h3>
          {loadingSharedMedia ? (
            <div className="loader-surface compact" role="status" aria-label="Carregando mídia compartilhada">
              <span className="modern-loader small" />
            </div>
          ) : sharedMedia.length > 0 ? (
            <div className="info-media-grid">
              {sharedMedia.map(media => (
                <div key={media.id} className="info-media-item">
                  <MessageMedia
                    chatId={selectedChat.id}
                    messageId={media.id}
                    isVideo={media.isVideo}
                    thumbnailUrl={media.thumbnailUrl}
                    palette={palette}
                    density={density}
                    downloadMeta={getDownloadMeta(media)}
                  />
                </div>
              ))}
            </div>
          ) : (
            <div className="info-media-grid-preview">
              <div style={{ padding: '20px 0', textAlign: 'center', color: 'var(--text-3)', fontSize: 12, border: '1px dashed var(--line-soft)', borderRadius: 8 }}>
                Nenhuma mídia encontrada
              </div>
            </div>
          )}
        </div>
      </>
    )}
  </div>
);
