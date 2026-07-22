import React from 'react';
import { ChatAvatar } from '../../components/ChatAvatar';
import type { Chat } from './TelegramDashboardTypes';
import { hashColor } from './TelegramDashboardConstants';
import { IconLogOut, IconSearch, IconSettings } from './DashboardIcons';
import { debugWarn } from '../../shared/debug/logger';

interface DashboardChatListProps {
  activeFolder: 'all' | 'unread';
  chatSearch: string;
  chats: Chat[];
  error: string;
  filteredChats: Chat[];
  isSearchOpen: boolean;
  isSettingsMenuOpen: boolean;
  loading: boolean;
  selectedChat: Chat | null;
  skipLogin: boolean;
  formatMessageTime: (timestamp: number) => string;
  getChatKind: (chat: Chat) => string;
  onTelegramLoginRequest?: () => void;
  readChatHistory: (chatId: string) => Promise<unknown>;
  setActiveFolder: React.Dispatch<React.SetStateAction<'all' | 'unread'>>;
  setChatContextMenu: React.Dispatch<React.SetStateAction<{ x: number; y: number; chat: Chat } | null>>;
  setChatSearch: React.Dispatch<React.SetStateAction<string>>;
  setChats: React.Dispatch<React.SetStateAction<Chat[]>>;
  setError: React.Dispatch<React.SetStateAction<string>>;
  setIsSearchOpen: React.Dispatch<React.SetStateAction<boolean>>;
  setIsSettingsMenuOpen: React.Dispatch<React.SetStateAction<boolean>>;
  setIsSettingsOpen: React.Dispatch<React.SetStateAction<boolean>>;
  setSelectedChat: React.Dispatch<React.SetStateAction<Chat | null>>;
}

export const DashboardChatList: React.FC<DashboardChatListProps> = ({
  activeFolder,
  chatSearch,
  chats,
  error,
  filteredChats,
  isSearchOpen,
  isSettingsMenuOpen,
  loading,
  selectedChat,
  skipLogin,
  formatMessageTime,
  getChatKind,
  onTelegramLoginRequest,
  readChatHistory,
  setActiveFolder,
  setChatContextMenu,
  setChatSearch,
  setChats,
  setError,
  setIsSearchOpen,
  setIsSettingsMenuOpen,
  setIsSettingsOpen,
  setSelectedChat,
}) => (
  <div className="list">
    <div className="list-header">
      <div className="list-title">
        <h1>Chats</h1>
        <div className="list-title-actions">
          <button
            className={`icon-btn ${isSearchOpen ? 'active' : ''}`}
            onClick={() => {
              setIsSearchOpen(value => !value);
              if (isSearchOpen) setChatSearch('');
            }}
            title={isSearchOpen ? 'Fechar pesquisa' : 'Pesquisar chats'}
          >
            <IconSearch />
          </button>
        </div>
      </div>
      {isSearchOpen && (
        <div className="search">
          <IconSearch />
          <input
            autoFocus
            type="text"
            value={chatSearch}
            onChange={event => setChatSearch(event.target.value)}
            placeholder="Pesquisar..."
          />
        </div>
      )}
    </div>

    <div className="folders">
      <div
        className={`folder ${activeFolder === 'all' ? 'active' : ''}`}
        onClick={() => setActiveFolder('all')}
      >
        Todos
        <span className="count">{chats.length}</span>
      </div>
      <div
        className={`folder ${activeFolder === 'unread' ? 'active' : ''}`}
        onClick={() => setActiveFolder('unread')}
      >
        Não lidos
        <span className="count">{chats.filter(chat => (chat.unreadCount ?? 0) > 0).length}</span>
      </div>
    </div>

    {error && (
      <div style={{ padding: '8px 12px', fontSize: 12, color: 'var(--danger)', background: 'color-mix(in oklch, var(--danger) 10%, var(--bg-1))' }}>
        {error}
      </div>
    )}

    <div className="chats">
      {skipLogin && (
        <div
          className="chat-row telegram-login-row"
          onClick={() => {
            setError('');
            onTelegramLoginRequest?.();
          }}
        >
          <div className="chat-avatar telegram-login-avatar">
            <IconLogOut />
          </div>
          <div className="telegram-login-copy">
            <div className="chat-name">
              <span className="name-text">Logar no Telegram</span>
            </div>
            <div className="chat-preview">
              Conectar sua conta para carregar chats reais
            </div>
          </div>
        </div>
      )}
      {filteredChats.map(chat => {
        const color = hashColor(chat.id);
        const hasUnread = (chat.unreadCount ?? 0) > 0;

        return (
          <div
            key={chat.id}
            className={`chat-row ${selectedChat?.id === chat.id ? 'active' : ''} ${hasUnread ? 'unread' : ''}`}
            onClick={() => {
              setSelectedChat(chat);
              setError('');
              if (hasUnread) {
                readChatHistory(chat.id).catch(debugWarn);
                setChats(prev => prev.map(item => item.id === chat.id ? { ...item, unreadCount: 0 } : item));
              }
            }}
            onContextMenu={event => {
              event.preventDefault();
              setChatContextMenu({ x: event.clientX, y: event.clientY, chat });
            }}
          >
            <div className={`chat-avatar color-${color}`}>
              <ChatAvatar chatId={chat.id} title={chat.title} />
            </div>
            <div className="chat-name">
              <span className="name-text">{chat.title || 'Unknown'}</span>
              {chat.hasTopics && <span className="badge-icon" title="Fórum">#</span>}
            </div>
            <span className="chat-meta">
              {chat.lastMessageDate ? formatMessageTime(chat.lastMessageDate) : ''}
            </span>
            <div className="chat-preview" style={{ display: 'flex', alignItems: 'center', gap: '4px' }}>
              {chat.lastMessageIsVideo && <span title="Vídeo">📹</span>}
              {chat.lastMessageIsPhoto && <span title="Foto">📷</span>}
              {!chat.lastMessageIsVideo && !chat.lastMessageIsPhoto && chat.lastMessageHasMedia && <span title="Mídia">📎</span>}
              <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                {chat.lastMessageText || getChatKind(chat)}
              </span>
            </div>
            <div className="chat-flags" style={{ gridColumn: 3 }}>
              {hasUnread && (
                <span className="chat-badge">
                  {chat.unreadCount! > 99 ? '99+' : chat.unreadCount}
                </span>
              )}
            </div>
          </div>
        );
      })}
      {!filteredChats.length && !loading && !skipLogin && (
        <div className="messages-empty">Nenhum chat encontrado.</div>
      )}
    </div>

    <div className="user-card">
      <div className="avatar">EU</div>
      <div>
        <div className="name">Você</div>
        <div className="sub"><span className="pip-dot" style={{ background: 'var(--good)', marginRight: 4 }} />online</div>
      </div>
      <div className="user-card-actions" style={{ position: 'relative' }}>
        <button
          className={`icon-btn ${isSettingsMenuOpen ? 'active' : ''}`}
          onClick={event => {
            event.stopPropagation();
            setIsSettingsMenuOpen(value => !value);
          }}
          title="Configurações e Aparência"
        >
          <IconSettings />
        </button>
        {isSettingsMenuOpen && (
          <div className="dropdown-menu" style={{ bottom: 'calc(100% + 8px)', top: 'auto', right: 0 }} onClick={event => event.stopPropagation()}>
            <div className="dropdown-item" onClick={() => { setIsSettingsOpen(true); setIsSettingsMenuOpen(false); }}>
              <IconSettings /> Configurações Gerais
            </div>
          </div>
        )}
      </div>
    </div>
  </div>
);
