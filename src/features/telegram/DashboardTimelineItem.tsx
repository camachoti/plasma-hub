import React, { useMemo } from 'react';
import { IconCornerUpLeft, IconCopy, IconDownload, IconMoodSmile, IconShare3 } from "../../design-system/icons";
import { ChatAvatar } from '../../components/ChatAvatar';
import { MessageMedia } from '../../components/MessageMedia';
import { writeClipboardText } from '../../shared/platform/clipboard';
import { debugWarn } from '../../shared/debug/logger';
import type { Density, Palette } from '../appearance/AppearanceStore';
import { hashColor } from './TelegramDashboardConstants';
import type { Chat, Message, TimelineItem } from './TelegramDashboardTypes';
import { telegramService } from './TelegramService';

const URL_REGEX = /(?:https?:\/\/|www\.)[^\s<>\u0000-\u001F\u007F\u00A0\u2000-\u200D\u2028\u2029\uFEFF]+|(?<![@\w.-])(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+(?:[a-z]{2,63})(?:\/[^\s<>\u0000-\u001F\u007F\u00A0\u2000-\u200D\u2028\u2029\uFEFF]*)?/gi;

const toExternalHref = (url: string) => (
  /^(?:https?:\/\/|tg:\/\/)/i.test(url) ? url : `https://${url}`
);

export const isTelegramLink = (url: string) => {
  if (url.startsWith('tg://')) return true;
  try {
    const lower = url.toLowerCase();
    if (lower.startsWith('tg:')) return true;
    const parsed = new URL(url.startsWith('http') ? url : `https://${url}`);
    const host = parsed.hostname.toLowerCase().replace(/^www\./, '');
    return host === 't.me' || host === 'telegram.me' || host === 'telegram.dog' || parsed.protocol === 'tg:';
  } catch {
    return url.toLowerCase().includes('t.me/') || url.toLowerCase().includes('telegram.me/');
  }
};

export const formatMessageTime = (timestamp: number) => {
  const d = new Date(timestamp * 1000);
  const now = new Date();
  const isToday = d.toDateString() === now.toDateString();
  const yesterday = new Date(now); yesterday.setDate(now.getDate() - 1);
  const isYesterday = d.toDateString() === yesterday.toDateString();
  if (isToday) return d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
  if (isYesterday) return 'Ontem';
  const daysAgo = (now.getTime() - d.getTime()) / 86400000;
  if (daysAgo < 7) return d.toLocaleDateString([], { weekday: 'short' });
  return d.toLocaleDateString([], { day: '2-digit', month: '2-digit' });
};

const formatMessageDate = (timestamp: number) =>
  new Date(timestamp * 1000).toLocaleDateString([], { weekday: 'short', day: 'numeric', month: 'short' });

const isNewMessageDay = (current: Message, previous?: Message) => {
  if (!previous) return true;
  return new Date(current.date * 1000).toDateString() !== new Date(previous.date * 1000).toDateString();
};

const isContinued = (msg: Message, prev?: Message) => {
  if (!prev) return false;
  if (prev.out !== msg.out) return false;
  if (!msg.out && prev.senderId !== msg.senderId) return false;
  if (isNewMessageDay(msg, prev)) return false;
  return msg.date - prev.date < 300;
};

export const getChatKind = (chat: Chat) => {
  if (chat.isFakeTwitter || (typeof chat.id === 'string' && chat.id.startsWith('twitter_profile_'))) return 'twitter';
  if (chat.isGroup) return 'grupo';
  if (chat.isChannel) return 'canal';
  return 'conversa';
};

const parseMessageIdKey = (value: string) => new Set(
  value ? value.split('|').map(id => Number(id)).filter(Number.isFinite) : []
);

interface TimelineMessageItemProps {
  item: TimelineItem;
  previousMessage?: Message;
  chatId: string;
  palette: Palette;
  density: Density;
  isSelectionMode: boolean;
  isHighlighted: boolean;
  selectedMediaIdsKey: string;
  visibleMediaIdsKey: string;
  repliedMessage?: Message;
  chatTitle?: string;
  chatKind?: string;
  topicTitle?: string;
  onTelegramLink: (url: string) => void;
  onClearEmojiPicker: () => void;
  onMessageContextMenu: (x: number, y: number, message: Message) => void;
  onUserContextMenu: (x: number, y: number, senderId: string, senderName: string) => void;
  onImageContextMenu: (x: number, y: number, message: Message) => void;
  onToggleSelectedMessage: (messageId: number) => void;
  onShowEmojiPicker: (messageId: number, rect: DOMRect) => void;
  onReact: (message: Message, emoji: string) => void;
  onReplyTo: (message: Message) => void;
  onForwardMessage: (message: Message) => void;
  onJumpToMessage: (messageId: number) => void;
}

export const TimelineMessageItem = React.memo(({
  item,
  previousMessage,
  chatId,
  palette,
  density,
  isSelectionMode,
  isHighlighted,
  selectedMediaIdsKey,
  visibleMediaIdsKey,
  repliedMessage,
  chatTitle,
  chatKind,
  topicTitle,
  onTelegramLink,
  onClearEmojiPicker,
  onMessageContextMenu,
  onUserContextMenu,
  onImageContextMenu,
  onToggleSelectedMessage,
  onShowEmojiPicker,
  onReact,
  onReplyTo,
  onForwardMessage,
  onJumpToMessage,
}: TimelineMessageItemProps) => {
  const msg = item.message;
  const continued = isContinued(msg, previousMessage);
  const dayBreak = isNewMessageDay(msg, previousMessage);
  const color = msg.out ? 'cyan' : hashColor(msg.senderId || msg.id.toString());
  const displayName = msg.out ? 'Você' : (msg.senderName || (msg.senderId ? `ID ${msg.senderId.slice(-6)}` : 'Desconhecido'));
  const selectedMediaIds = useMemo(() => parseMessageIdKey(selectedMediaIdsKey), [selectedMediaIdsKey]);
  const visibleMediaIds = useMemo(() => parseMessageIdKey(visibleMediaIdsKey), [visibleMediaIdsKey]);
  const albumMedias = useMemo(
    () => item.messages?.map(m => ({ id: m.id, isVideo: m.isVideo, videoDuration: m.videoDuration, messageDate: m.date, mediaSize: m.mediaSize })),
    [item.messages]
  );
  const textContent = useMemo(() => {
    if (!msg.text) return null;
    const parts: (string | React.ReactElement)[] = [];
    let lastIndex = 0;
    let match: RegExpExecArray | null;
    const regex = new RegExp(URL_REGEX.source, 'g');
    while ((match = regex.exec(msg.text)) !== null) {
      if (match.index > lastIndex) parts.push(msg.text.slice(lastIndex, match.index));
      const url = match[0];
      const trailing = url.match(/[)\]}"',;.!?]+$/);
      const cleanUrl = trailing ? url.slice(0, url.length - trailing[0].length) : url;
      const displayUrl = cleanUrl.length > 60 ? cleanUrl.slice(0, 57) + '...' : cleanUrl;
      const href = toExternalHref(cleanUrl);
      if (isTelegramLink(href)) {
        parts.push(<a key={match.index} href="#" onClick={e => { e.preventDefault(); onTelegramLink(href); }} className="message-link message-link-tg">{displayUrl}</a>);
      } else {
        parts.push(<a key={match.index} href={href} target="_blank" rel="noopener noreferrer" className="message-link">{displayUrl}</a>);
      }
      lastIndex = match.index + url.length;
    }
    if (lastIndex < msg.text.length) parts.push(msg.text.slice(lastIndex));
    return parts;
  }, [msg.text, onTelegramLink]);
  const buildDownloadMeta = (message: Message, senderName?: string | null) => ({
    chatTitle,
    chatKind,
    topicTitle,
    senderName: senderName || (message.out ? 'Você' : message.senderName || undefined),
    senderId: message.senderId,
  });
  const showEmojiPicker = (messageId: number, currentTarget: HTMLButtonElement) => {
    onShowEmojiPicker(messageId, currentTarget.getBoundingClientRect());
  };

  return (
    <div style={{ paddingBottom: 'var(--msg-gap)' }}>
      {dayBreak && (
        <div className="day-divider">
          <div className="line" />
          <div className="label">{formatMessageDate(msg.date)}</div>
          <div className="line" />
        </div>
      )}
      <div
        id={`msg-${msg.id}`}
        data-msg-id={msg.id}
        className={`msg-row ${msg.out ? 'self' : ''} ${continued ? 'continued' : ''}${msg.isDeleted ? ' msg-deleted' : ''}`}
        onClick={onClearEmojiPicker}
        onContextMenu={(e) => { e.preventDefault(); e.stopPropagation(); onMessageContextMenu(e.clientX, e.clientY, msg); }}
      >
        {!msg.out && (
          <div
            className={`msg-avatar color-${color}`}
            style={continued ? { visibility: 'hidden' } : undefined}
            onContextMenu={(e) => {
              if (msg.senderId) {
                e.preventDefault();
                e.stopPropagation();
                onUserContextMenu(e.clientX, e.clientY, msg.senderId, displayName);
              }
            }}
          >
            <ChatAvatar chatId={msg.senderId || ''} title={displayName} />
          </div>
        )}
        <div className="msg-body">
          {!continued && (
            <div className="msg-head">
              <span
                className={`msg-from color-${color}`}
                onContextMenu={(e) => {
                  if (msg.senderId) {
                    e.preventDefault();
                    e.stopPropagation();
                    onUserContextMenu(e.clientX, e.clientY, msg.senderId, displayName);
                  }
                }}
              >
                {displayName}
              </span>
              <span className="msg-time">
                {formatMessageTime(msg.date)}
                {msg.is_edited && <span className="msg-edited-badge">(editado)</span>}
              </span>
            </div>
          )}
          {continued && (
            <div className="msg-time-inline">
              {formatMessageTime(msg.date)}
              {msg.is_edited && <span className="msg-edited-badge">(editado)</span>}
            </div>
          )}
          <div className={`msg-bubble ${!msg.text && (msg.hasMedia || item.type === 'album') ? 'media-only' : ''} ${isHighlighted ? 'highlight-flash' : ''}`}>
            {msg.replyToMsgId && (() => {
              const sender = repliedMessage
                ? (repliedMessage.out ? 'Você' : (repliedMessage.senderName || 'Desconhecido'))
                : `Mensagem #${msg.replyToMsgId}`;
              const text = repliedMessage
                ? (repliedMessage.text ? repliedMessage.text.slice(0, 60) : (repliedMessage.hasMedia ? '📷 Mídia' : ''))
                : 'Clique para saltar para a mensagem';

              return (
                <div className="msg-reply-preview" onClick={(e) => { e.stopPropagation(); onJumpToMessage(msg.replyToMsgId!); }}>
                  <div className="reply-preview-bar" />
                  <div className="reply-preview-content">
                    <span className="reply-preview-sender">{sender}</span>
                    <span className="reply-preview-text">{text}</span>
                  </div>
                </div>
              );
            })()}
            {item.type === 'album' ? (
              <div className={`album-grid album-grid-${Math.min(9, item.messages!.length)}`}>
                {item.messages!.map(albumMsg => (
                  <div
                    key={albumMsg.id}
                    className={`album-grid-item msg-media ${selectedMediaIds.has(albumMsg.id) ? 'media-selected' : ''}`}
                    onContextMenu={(e) => { e.preventDefault(); e.stopPropagation(); onImageContextMenu(e.clientX, e.clientY, albumMsg); }}
                  >
                    <MessageMedia
                      chatId={chatId}
                      messageId={albumMsg.id}
                      isVideo={albumMsg.isVideo}
                      videoDuration={albumMsg.videoDuration}
                      messageDate={albumMsg.date}
                      mediaSize={albumMsg.mediaSize}
                      thumbnailUrl={albumMsg.thumbnailUrl}
                      mediaPriority={visibleMediaIds.has(albumMsg.id) ? 'visible' : 'background'}
                      palette={palette}
                      density={density}
                      downloadMeta={buildDownloadMeta(albumMsg, albumMsg.out ? 'Você' : albumMsg.senderName || displayName)}
                      selectionMode={isSelectionMode}
                      onClickOverride={isSelectionMode ? () => onToggleSelectedMessage(albumMsg.id) : undefined}
                      albumMedias={albumMedias}
                    />
                  </div>
                ))}
              </div>
            ) : (
              msg.hasMedia && (
                <div
                  className={`msg-media ${selectedMediaIds.has(msg.id) ? 'media-selected' : ''}`}
                  onContextMenu={(e) => { e.preventDefault(); e.stopPropagation(); onImageContextMenu(e.clientX, e.clientY, msg); }}
                >
                  <MessageMedia
                    chatId={chatId}
                    messageId={msg.id}
                    isVideo={msg.isVideo}
                    videoDuration={msg.videoDuration}
                    messageDate={msg.date}
                    mediaSize={msg.mediaSize}
                    thumbnailUrl={msg.thumbnailUrl}
                    mediaPriority={visibleMediaIds.has(msg.id) ? 'visible' : 'background'}
                    palette={palette}
                    density={density}
                    downloadMeta={buildDownloadMeta(msg, displayName)}
                    selectionMode={isSelectionMode}
                    onClickOverride={isSelectionMode ? () => onToggleSelectedMessage(msg.id) : undefined}
                  />
                </div>
              )
            )}
            {textContent && <div className="msg-text">{textContent}</div>}
            {msg.isDeleted && (
              <div className="msg-deleted-badge">🗑️ Mensagem excluída no servidor</div>
            )}
          </div>
          {msg.reactions && msg.reactions.length > 0 && (
            <div className="reactions">
              {msg.reactions.map((reaction, index) => (
                <button key={index} type="button" className={`reaction ${reaction.mine ? 'mine' : ''}`} onClick={() => onReact(msg, reaction.emoji)} aria-label={`Reagir com ${reaction.emoji}, ${reaction.count} reações`} aria-pressed={reaction.mine}>
                  <span>{reaction.emoji}</span>
                  <span>{reaction.count}</span>
                </button>
              ))}
              <button
                className="reaction-add"
                title="Reagir"
                aria-label="Adicionar reação"
                onClick={(e) => { e.stopPropagation(); showEmojiPicker(msg.id, e.currentTarget); }}
              >+</button>
            </div>
          )}
        </div>
        <div className="msg-actions">
          <button
            type="button" className="icon-btn" title="Reagir" aria-label="Reagir à mensagem"
            onClick={(e) => { e.stopPropagation(); showEmojiPicker(msg.id, e.currentTarget); }}
          ><IconMoodSmile size={18} stroke={2} /></button>
          <button
            type="button" className="icon-btn" title="Responder" aria-label="Responder à mensagem"
            onClick={(e) => { e.stopPropagation(); onReplyTo(msg); }}
          ><IconCornerUpLeft size={18} stroke={2} /></button>
          <button
            type="button" className="icon-btn" title="Encaminhar" aria-label="Encaminhar mensagem"
            onClick={(e) => { e.stopPropagation(); onForwardMessage(msg); }}
          ><IconShare3 size={18} stroke={2} /></button>
          {item.type === 'album' ? (
            <button
              type="button" className="icon-btn" title="Salvar todas as mídias" aria-label="Salvar todas as mídias do álbum"
              onClick={async (e) => {
                e.stopPropagation();
                if (item.messages) {
                  for (const albumMsg of item.messages) {
                    try {
                      await telegramService.saveMessageMediaFile({
                        chatId,
                        messageId: albumMsg.id,
                        downloadMeta: buildDownloadMeta(albumMsg, albumMsg.out ? 'Você' : albumMsg.senderName || displayName),
                      });
                    } catch (err) {
                      debugWarn(err);
                    }
                  }
                }
              }}
            ><IconDownload size={18} stroke={2} /></button>
          ) : msg.hasMedia ? (
            <button
              type="button" className="icon-btn" title="Salvar" aria-label="Salvar mídia"
              onClick={(e) => {
                e.stopPropagation();
                telegramService.saveMessageMediaFile({
                  chatId,
                  messageId: msg.id,
                  downloadMeta: buildDownloadMeta(msg, displayName),
                });
              }}
            ><IconDownload size={18} stroke={2} /></button>
          ) : msg.text ? (
            <button
              type="button" className="icon-btn" title="Copiar texto" aria-label="Copiar texto da mensagem"
              onClick={(e) => { e.stopPropagation(); writeClipboardText(msg.text); }}
            ><IconCopy size={18} stroke={2} /></button>
          ) : null}
        </div>
      </div>
    </div>
  );
});
