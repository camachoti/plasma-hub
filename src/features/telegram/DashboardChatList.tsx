import React from 'react';
import { Virtuoso } from 'react-virtuoso';
import { ChatAvatar } from '../../components/ChatAvatar';
import { ChatListSkeleton } from '../../components/Skeletons';
import type { Chat } from './TelegramDashboardTypes';
import { hashColor } from './TelegramDashboardConstants';
import { IconLogOut, IconSearch } from './DashboardIcons';
import { debugWarn } from '../../shared/debug/logger';

interface DashboardChatListProps {
  activeFolder: 'all' | 'unread';
  chatSearch: string;
  chats: Chat[];
  error: string;
  filteredChats: Chat[];
  isSearchOpen: boolean;
  loading: boolean;
  loadingMore: boolean;
  hasMoreChats: boolean;
  isRefreshing: boolean;
  selectedChat: Chat | null;
  skipLogin: boolean;
  unreadChatsCount: number;
  formatMessageTime: (timestamp: number) => string;
  getChatKind: (chat: Chat) => string;
  onTelegramLoginRequest?: () => void;
  onLoadMoreChats: () => void;
  readChatHistory: (chatId: string) => Promise<unknown>;
  setActiveFolder: React.Dispatch<React.SetStateAction<'all' | 'unread'>>;
  setChatContextMenu: React.Dispatch<React.SetStateAction<{ x: number; y: number; chat: Chat } | null>>;
  setChatSearch: React.Dispatch<React.SetStateAction<string>>;
  setChats: React.Dispatch<React.SetStateAction<Chat[]>>;
  setError: React.Dispatch<React.SetStateAction<string>>;
  setIsSearchOpen: React.Dispatch<React.SetStateAction<boolean>>;
  setSelectedChat: React.Dispatch<React.SetStateAction<Chat | null>>;
}

interface ChatRowProps {
  chat: Chat;
  isActive: boolean;
  formatMessageTime: (timestamp: number) => string;
  getChatKind: (chat: Chat) => string;
  readChatHistory: (chatId: string) => Promise<unknown>;
  setChatContextMenu: React.Dispatch<React.SetStateAction<{ x: number; y: number; chat: Chat } | null>>;
  setChats: React.Dispatch<React.SetStateAction<Chat[]>>;
  setError: React.Dispatch<React.SetStateAction<string>>;
  setSelectedChat: React.Dispatch<React.SetStateAction<Chat | null>>;
}

const ChatRow = React.memo(({
  chat,
  isActive,
  formatMessageTime,
  getChatKind,
  readChatHistory,
  setChatContextMenu,
  setChats,
  setError,
  setSelectedChat,
}: ChatRowProps) => {
  const color = hashColor(chat.id);
  const hasUnread = (chat.unreadCount ?? 0) > 0;

  return (
    <div
      className={`chat-row ${isActive ? 'active' : ''} ${hasUnread ? 'unread' : ''}`}
      role="button"
      tabIndex={0}
      aria-current={isActive ? 'page' : undefined}
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
      onKeyDown={event => {
        if (event.key === 'Enter' || event.key === ' ') {
          event.preventDefault();
          event.currentTarget.click();
        }
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
});

const DashboardChatListComponent: React.FC<DashboardChatListProps> = ({
  activeFolder,
  chatSearch,
  chats,
  error,
  filteredChats,
  isSearchOpen,
  loading,
  loadingMore,
  hasMoreChats,
  isRefreshing,
  selectedChat,
  skipLogin,
  unreadChatsCount,
  formatMessageTime,
  getChatKind,
  onTelegramLoginRequest,
  onLoadMoreChats,
  readChatHistory,
  setActiveFolder,
  setChatContextMenu,
  setChatSearch,
  setChats,
  setError,
  setIsSearchOpen,
  setSelectedChat,
}) => (
  <div className="list">
    <div className="list-header">
      <div className="list-title">
        <div className="list-title-copy">
          <span className="list-overline">
            Telegram
            {isRefreshing && <em aria-live="polite">Atualizando</em>}
          </span>
          <h1>Conversas</h1>
        </div>
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
            aria-label="Pesquisar na lista de chats e grupos"
          />
        </div>
      )}
    </div>

    <div className="folders">
      <div
        className={`folder ${activeFolder === 'all' ? 'active' : ''}`}
        role="button"
        tabIndex={0}
        aria-pressed={activeFolder === 'all'}
        onClick={() => setActiveFolder('all')}
        onKeyDown={event => (event.key === 'Enter' || event.key === ' ') && event.currentTarget.click()}
      >
        Todos
        <span className="count">{chats.length}</span>
      </div>
      <div
        className={`folder ${activeFolder === 'unread' ? 'active' : ''}`}
        role="button"
        tabIndex={0}
        aria-pressed={activeFolder === 'unread'}
        onClick={() => setActiveFolder('unread')}
        onKeyDown={event => (event.key === 'Enter' || event.key === ' ') && event.currentTarget.click()}
      >
        Não lidos
        <span className="count">{unreadChatsCount}</span>
      </div>
    </div>

    {error && (
      <div style={{ padding: '8px 12px', fontSize: 12, color: 'var(--danger)', background: 'color-mix(in oklch, var(--danger) 10%, var(--bg-1))' }}>
        {error}
      </div>
    )}

    <Virtuoso
      className="chats"
      style={{ minHeight: 0 }}
      data={filteredChats}
      endReached={() => {
        if (hasMoreChats && !loadingMore && !chatSearch.trim() && activeFolder === 'all') {
          onLoadMoreChats();
        }
      }}
      computeItemKey={(_, chat) => chat.id}
      components={{
        Header: () => skipLogin ? (
          <div className="chat-list-item">
            <div
              className="chat-row telegram-login-row"
              role="button"
              tabIndex={0}
              onClick={() => {
                setError('');
                onTelegramLoginRequest?.();
              }}
              onKeyDown={event => (event.key === 'Enter' || event.key === ' ') && event.currentTarget.click()}
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
          </div>
        ) : null,
        EmptyPlaceholder: () => loading ? <ChatListSkeleton /> : null,
        Footer: () => loadingMore ? (
          <div className="chat-list-loading-more" role="status">Carregando mais conversas…</div>
        ) : !filteredChats.length && !loading && !skipLogin ? (
          <div className="chat-list-empty-state" role="status">
            <div className="chat-list-empty-icon" aria-hidden="true">⌁</div>
            <strong>{chatSearch.trim() ? 'Nenhum chat encontrado' : activeFolder === 'unread' ? 'Tudo em dia' : 'Nenhuma conversa disponível'}</strong>
            <span>
              {chatSearch.trim()
                ? `Não há conversas correspondentes a “${chatSearch.trim()}”.`
                : activeFolder === 'unread'
                  ? 'Você não possui mensagens não lidas.'
                  : 'As conversas carregadas aparecerão aqui.'}
            </span>
            {chatSearch.trim() ? (
              <button type="button" onClick={() => setChatSearch('')}>Limpar pesquisa</button>
            ) : activeFolder === 'unread' ? (
              <button type="button" onClick={() => setActiveFolder('all')}>Mostrar todos</button>
            ) : null}
          </div>
        ) : null,
      }}
      itemContent={(_, chat) => (
        <div className="chat-list-item">
          <ChatRow
            chat={chat}
            isActive={selectedChat?.id === chat.id}
            formatMessageTime={formatMessageTime}
            getChatKind={getChatKind}
            readChatHistory={readChatHistory}
            setChatContextMenu={setChatContextMenu}
            setChats={setChats}
            setError={setError}
            setSelectedChat={setSelectedChat}
          />
        </div>
      )}
    />

    <div className="user-card">
      <div className="avatar">EU</div>
      <div>
        <div className="name">Você</div>
        <div className="sub"><span className="pip-dot" style={{ background: 'var(--good)', marginRight: 4 }} />online</div>
      </div>
    </div>
  </div>
);

export const DashboardChatList = React.memo(DashboardChatListComponent);
