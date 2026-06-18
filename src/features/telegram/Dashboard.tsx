// @ts-nocheck
import React, { useEffect, useState, useRef, useCallback } from 'react';
import '../../styles/Dashboard.css';
import { ChatAvatar } from '../../components/ChatAvatar';
import { MessageMedia } from '../../components/MessageMedia';
import { ContextMenu, IcoDownload, IcoCopy, IcoForward, IcoReply } from '../../components/ContextMenu';
import { telegramService } from './TelegramService';
import { Settings } from './Settings';
import { Virtuoso } from 'react-virtuoso';
import appIcon from '../../../build/icon.png';

const QUICK_REACTIONS = ['👍', '❤️', '😂', '🔥', '🥰', '👏', '😁', '🤔', '🤯', '😱', '😢', '🎉', '🙏', '👌', '💯', '🤣', '🤩', '🤮', '💩', '🖕', '😈'];

const TOPIC_ICON_COLORS = [
  { label: 'Vermelho', value: 16711680, css: '#ff4444' },
  { label: 'Laranja', value: 16744272, css: '#ff9010' },
  { label: 'Violeta', value: 7322096, css: '#6f48eb' },
  { label: 'Verde', value: 528304, css: '#00a152' },
  { label: 'Ciano', value: 3284671, css: '#32b3ff' },
  { label: 'Rosa', value: 14318475, css: '#da8aff' },
];

const PLASMA_COLORS = ['rose', 'violet', 'cyan', 'amber', 'emerald', 'fuchsia', 'sky'] as const;
type PlasmaColor = typeof PLASMA_COLORS[number];

const hashColor = (str: string | undefined | null): PlasmaColor => {
  if (!str || typeof str !== 'string') return 'cyan';
  let h = 0;
  for (let i = 0; i < str.length; i++) h = ((h << 5) - h + str.charCodeAt(i)) & 0x7fffffff;
  return PLASMA_COLORS[Math.abs(h) % PLASMA_COLORS.length];
};

const PALETTES = ['abyssal', 'ion', 'ember', 'bloom', 'forest', 'light'] as const;
type Palette = typeof PALETTES[number];

const DENSITIES = ['compact', 'cozy', 'roomy'] as const;
type Density = typeof DENSITIES[number];

interface Chat {
  id: string;
  title: string;
  isGroup: boolean;
  isChannel: boolean;
  isMember?: boolean;
  isInvite?: boolean;
  inviteHash?: string;
  about?: string;
  participantsCount?: number;
  hasTopics?: boolean;
  lastMessageText?: string;
  lastMessageDate?: number;
  unreadCount?: number;
  lastMessageHasMedia?: boolean;
  lastMessageIsVideo?: boolean;
  lastMessageIsPhoto?: boolean;
}

interface ChatFullInfo {
  about?: string;
  participantsCount?: number;
  username?: string | null;
  pinnedMsgId?: number | null;
}

interface ForumTopic {
  id: number;
  title: string;
  topMessageId: number;
  unreadCount: number;
  closed: boolean;
  pinned: boolean;
}

interface Message {
  id: number;
  text: string;
  date: number;
  out: boolean;
  senderId: string | null;
  senderName?: string | null;
  hasMedia: boolean;
  isPhoto: boolean;
  isVideo: boolean;
  videoDuration?: number | null;
  mediaSize?: number | null;
  reactions?: Array<{ emoji: string; count: number; mine: boolean }>;
  is_deleted?: boolean;
  is_edited?: boolean;
  replyToMsgId?: number | null;
  groupedId?: string | null;
}

interface TimelineItem {
  type: 'message' | 'album';
  id: number;
  message: Message;
  messages?: Message[];
}

const getTimelineItems = (msgs: Message[]): TimelineItem[] => {
  const items: TimelineItem[] = [];
  let currentAlbum: { id: string; messages: Message[] } | null = null;

  for (const m of msgs) {
    if (m.groupedId) {
      if (currentAlbum && currentAlbum.id === m.groupedId) {
        currentAlbum.messages.push(m);
      } else {
        if (currentAlbum) {
          items.push({
            type: 'album',
            id: currentAlbum.messages[0].id,
            message: currentAlbum.messages.find(msg => msg.text) || currentAlbum.messages[0],
            messages: [...currentAlbum.messages]
          });
        }
        currentAlbum = { id: m.groupedId, messages: [m] };
      }
    } else {
      if (currentAlbum) {
        items.push({
          type: 'album',
          id: currentAlbum.messages[0].id,
          message: currentAlbum.messages.find(msg => msg.text) || currentAlbum.messages[0],
          messages: [...currentAlbum.messages]
        });
        currentAlbum = null;
      }
      items.push({
        type: 'message',
        id: m.id,
        message: m
      });
    }
  }

  if (currentAlbum) {
    items.push({
      type: 'album',
      id: currentAlbum.messages[0].id,
      message: currentAlbum.messages.find(msg => msg.text) || currentAlbum.messages[0],
      messages: [...currentAlbum.messages]
    });
  }

  return items;
};

interface DownloadItem {
  name: string;
  status: 'downloading' | 'completed' | 'skipped' | 'failed';
  progress: number;
  size: number;
}

interface DownloadProgress {
  total: number;
  downloaded: number;
  currentFile: string;
  topicTitle?: string | null;
  isScanning?: boolean;
  items?: DownloadItem[];
}

const formatBytes = (bytes: number) => {
  if (bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i];
};

// Icon helpers
const IconSearch = () => (
  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    <circle cx="11" cy="11" r="6.5" /><path d="m20 20-3.5-3.5" />
  </svg>
);
const IconMore = () => (
  <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    <circle cx="6" cy="12" r="1.2" /><circle cx="12" cy="12" r="1.2" /><circle cx="18" cy="12" r="1.2" />
  </svg>
);
const IconPanel = () => (
  <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    <rect x="3" y="4" width="18" height="16" rx="2" /><path d="M15 4v16" />
  </svg>
);
const IconPlus = () => (
  <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
    <path d="M12 5v14M5 12h14" />
  </svg>
);
const IconBack = () => (
  <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
    <path d="M15 18 9 12l6-6" />
  </svg>
);
const IconSend = () => (
  <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
    <path d="M4 12 20 5l-4 15-4-7-8-1Z" />
  </svg>
);
const IconAttach = () => (
  <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    <path d="M20 11.5 11.5 20a5 5 0 0 1-7-7l9-9a3.5 3.5 0 0 1 5 5l-9 9a2 2 0 0 1-3-3l8-8" />
  </svg>
);
const IconEmoji = () => (
  <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    <circle cx="12" cy="12" r="9" /><path d="M8.5 14a4 4 0 0 0 7 0M9 9.5h.01M15 9.5h.01" />
  </svg>
);
const IconSettings = () => (
  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    <path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.1a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" />
    <circle cx="12" cy="12" r="3" />
  </svg>
);
const IconCheck = () => (
  <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
    <path d="M20 6 9 17l-5-5" />
  </svg>
);
const IconBell = () => (
  <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    <path d="M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9m4.35 13a2 2 0 0 0 3.3 0" />
  </svg>
);
const IconLogOut = () => (
  <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    <path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4m7 14 5-5-5-5m5 5H9" />
  </svg>
);
const IconTrash = () => (
  <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    <path d="M3 6h18m-2 0v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6m3 0V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2" />
  </svg>
);
const IconMagic = () => (
  <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
    <polyline points="12 11 12 19 9 16" />
    <line x1="15" x2="12" y1="16" y2="19" />
  </svg>
);
const IconDownload = () => (
  <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
    <polyline points="7 10 12 15 17 10" />
    <line x1="12" x2="12" y1="15" y2="3" />
  </svg>
);

const topicColors = [
  '#5ca9e6, #7d95ff',
  '#e8786e, #f5a623',
  '#6ec6b8, #43a047',
  '#ab7ae6, #e66fa0',
  '#f5a623, #f7c948',
  '#5cb8e6, #4fc3f7',
  '#e66fa0, #ef5350',
  '#66bb6a, #aed581',
];
const getTopicColor = (id: number) => topicColors[Math.abs(id) % topicColors.length];

const ListContainer = React.forwardRef<HTMLDivElement, any>(({ style, children, ...props }, ref) => {
  const getPadding = (baseVal: any, extraPx: number) => {
    if (baseVal === undefined || baseVal === null) return `${extraPx}px`;
    if (typeof baseVal === 'number') return `${baseVal + extraPx}px`;
    return `calc(${baseVal} + ${extraPx}px)`;
  };

  return (
    <div
      {...props}
      ref={ref}
      style={{
        ...style,
        paddingTop: getPadding(style?.paddingTop, 18),
        paddingBottom: getPadding(style?.paddingBottom, 32)
      }}
    >
      {children}
    </div>
  );
});

interface DashboardProps {
  skipLogin?: boolean;
}

export const Dashboard: React.FC<DashboardProps> = ({ skipLogin = false }) => {
  const PAGE_SIZE = 50;
  const [chats, setChats] = useState<Chat[]>([]);
  const [loading, setLoading] = useState(true);
  const [selectedChat, setSelectedChat] = useState<Chat | null>(null);
  const [messages, setMessages] = useState<Message[]>([]);
  const [loadingMessages, setLoadingMessages] = useState(false);
  const [loadingMoreMessages, setLoadingMoreMessages] = useState(false);
  const [hasMoreMessages, setHasMoreMessages] = useState(false);
  const [oldestMessageId, setOldestMessageId] = useState<number | null>(null);
  const virtuosoRef = useRef<any>(null);
  const messagesRef = useRef<Message[]>([]);
  const timelineRef = useRef<HTMLDivElement>(null);
  const topicListRef = useRef<HTMLDivElement>(null);
  const shouldScrollToBottomRef = useRef(false);
  const preserveScrollPositionRef = useRef<number | null>(null);
  const topicListScrollRef = useRef<number>(0);
  const pendingJumpToMsgIdRef = useRef<number | null>(null);
  const progressDetailsListRef = useRef<HTMLDivElement | null>(null);

  const [isDownloadModalOpen, setIsDownloadModalOpen] = useState(false);
  const [isMenuOpen, setIsMenuOpen] = useState(false);
  const [forumTopics, setForumTopics] = useState<ForumTopic[]>([]);
  const [loadingTopics, setLoadingTopics] = useState(false);
  const [selectedTopicId, setSelectedTopicId] = useState<string>('all');
  const [viewingTopic, setViewingTopic] = useState<ForumTopic | null>(null);

  const [folderPath, setFolderPath] = useState<string>('');
  const [splitByUser, setSplitByUser] = useState<boolean>(false);
  const [isSelectionMode, setIsSelectionMode] = useState<boolean>(false);
  const [selectedMessageIds, setSelectedMessageIds] = useState<number[]>([]);
  const [progress, setProgress] = useState<DownloadProgress | null>(null);
  const [downloading, setDownloading] = useState(false);
  const [stopping, setStopping] = useState(false);
  const [error, setError] = useState('');
  const [isSearchOpen, setIsSearchOpen] = useState(false);
  const [chatSearch, setChatSearch] = useState('');
  const [topicSearch, setTopicSearch] = useState('');
  const [isTopicDropdownOpen, setIsTopicDropdownOpen] = useState(false);

  const [inputText, setInputText] = useState('');
  const [replyTo, setReplyTo] = useState<Message | null>(null);
  const [selectedFile, setSelectedFile] = useState<{ filePath: string; fileName: string } | null>(null);
  const [sendProgress, setSendProgress] = useState<number | null>(null);
  const [isSending, setIsSending] = useState(false);
  const [msgContextMenu, setMsgContextMenu] = useState<{ x: number; y: number; message: Message } | null>(null);
  const [chatContextMenu, setChatContextMenu] = useState<{ x: number; y: number; chat: Chat } | null>(null);
  const [imgContextMenu, setImgContextMenu] = useState<{ x: number; y: number; msg: Message } | null>(null);
  const [userContextMenu, setUserContextMenu] = useState<{ x: number; y: number; senderId: string; senderName: string } | null>(null);
  const [searchMediaUser, setSearchMediaUser] = useState<{ senderId: string; senderName: string } | null>(null);
  const [searchMediaResults, setSearchMediaResults] = useState<any[]>([]);
  const [searchMediaLoading, setSearchMediaLoading] = useState(false);
  const [showDetailedProgress, setShowDetailedProgress] = useState(false);
  const [bulkDownloadActive, setBulkDownloadActive] = useState(false);
  const [bulkProgress, setBulkProgress] = useState<{
    total: number;
    downloaded: number;
    currentFile: string;
    status: string;
  } | null>(null);
  const [isCreatingTopic, setIsCreatingTopic] = useState(false);
  const [newTopicTitle, setNewTopicTitle] = useState('');
  const [newTopicColor, setNewTopicColor] = useState(7322096);

  const [palette, setPalette] = useState<Palette>('abyssal');
  const [density, setDensity] = useState<Density>('cozy');
  const [infoOpen, setInfoOpen] = useState(false);
  const [emojiPickerMsgId, setEmojiPickerMsgId] = useState<number | null>(null);
  const [emojiPickerPos, setEmojiPickerPos] = useState<{ x: number; y: number }>({ x: 0, y: 0 });
  const [originalMsgModal, setOriginalMsgModal] = useState<{ msg: Message; original: { id: number; text: string; date: number } } | null>(null);
  const [highlightedMsgId, setHighlightedMsgId] = useState<number | null>(null);
  const [activeFolder, setActiveFolder] = useState<'all' | 'unread'>('all');
  const [isSettingsOpen, setIsSettingsOpen] = useState(false);
  const [isDraggingOver, setIsDraggingOver] = useState(false);
  const [isSettingsMenuOpen, setIsSettingsMenuOpen] = useState(false);
  const [fullChatInfo, setFullChatInfo] = useState<ChatFullInfo | null>(null);
  const [loadingFullInfo, setLoadingFullInfo] = useState(false);
  const [sharedMedia, setSharedMedia] = useState<any[]>([]);
  const [loadingSharedMedia, setLoadingSharedMedia] = useState(false);
  const [confirmModal, setConfirmModal] = useState<{
    title: string;
    body: string;
    danger?: boolean;
    hideCancel?: boolean;
    confirmText?: string;
    onConfirm: () => void;
  } | null>(null);

  const filteredChats = chats.filter(chat => {
    const matchesSearch = chat.title.toLowerCase().includes(chatSearch.trim().toLowerCase());
    if (activeFolder === 'unread') {
      return matchesSearch && (chat.unreadCount ?? 0) > 0;
    }
    return matchesSearch;
  });
  const filteredTopics = forumTopics.filter(topic =>
    topic.title.toLowerCase().includes(topicSearch.trim().toLowerCase())
  );

  const getChatKind = (chat: Chat) => {
    if (chat.isGroup) return 'grupo';
    if (chat.isChannel) return 'canal';
    return 'conversa';
  };

  const formatMessageTime = (timestamp: number) => {
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

  const URL_REGEX = /(https?:\/\/[^\s<>\u0000-\u001F\u007F\u00A0\u2000-\u200D\u2028\u2029\uFEFF]+)/g;

  const isTelegramLink = (url: string) => {
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

  const handleTelegramLinkRef = useRef<((url: string) => void) | null>(null);

  const handleTelegramLink = async (url: string) => {
    try {
      const lowerUrl = url.toLowerCase();
      const isInvite = lowerUrl.includes('t.me/+') || lowerUrl.includes('/joinchat/') || lowerUrl.includes('/invite/') || lowerUrl.includes('invite=');

      if (isInvite) {
        const inviteRes = await telegramService.checkInvite(url);
        if (inviteRes.success && inviteRes.chat) {
          if (inviteRes.alreadyMember) {
            const existing = chats.find(c => c.id === inviteRes.chat!.id);
            if (!existing) setChats(prev => [inviteRes.chat!, ...prev]);
            setSelectedChat(inviteRes.chat!);
          } else {
            setSelectedChat(inviteRes.chat!);
          }
        } else {
          setConfirmModal({
            title: 'Erro',
            body: inviteRes.error || 'Não foi possível obter informações do convite.',
            hideCancel: true,
            confirmText: 'Fechar',
            onConfirm: () => setConfirmModal(null)
          });
        }
        return;
      }

      const res = await telegramService.resolveLink(url);
      if (res.success && res.chat) {
        const existing = chats.find(c => c.id === res.chat!.id);
        if (!existing) setChats(prev => [res.chat!, ...prev]);
        setSelectedChat(res.chat!);
      } else {
        if (isTelegramLink(url)) {
          setConfirmModal({
            title: 'Não encontrado',
            body: `Não conseguimos encontrar este chat no Telegram: ${res.error || 'Erro desconhecido'}`,
            hideCancel: true,
            confirmText: 'Fechar',
            onConfirm: () => setConfirmModal(null)
          });
        } else {
          telegramService.openExternal(url);
        }
      }
    } catch (err) {
      console.error('Handle link error:', err);
      telegramService.openExternal(url);
    }
  };

  const linkifyText = (text: string) => {
    const parts: (string | React.ReactElement)[] = [];
    let lastIndex = 0;
    let match: RegExpExecArray | null;
    const regex = new RegExp(URL_REGEX.source, 'g');
    while ((match = regex.exec(text)) !== null) {
      if (match.index > lastIndex) parts.push(text.slice(lastIndex, match.index));
      const url = match[1];
      const trailing = url.match(/[)\]}"',;.!?]+$/);
      const cleanUrl = trailing ? url.slice(0, url.length - trailing[0].length) : url;
      const displayUrl = cleanUrl.length > 60 ? cleanUrl.slice(0, 57) + '...' : cleanUrl;
      const isTg = isTelegramLink(cleanUrl);
      if (isTg) {
        parts.push(<a key={match.index} href="#" onClick={e => { e.preventDefault(); handleTelegramLink(cleanUrl); }} className="message-link message-link-tg">{displayUrl}</a>);
      } else {
        parts.push(<a key={match.index} href={cleanUrl} target="_blank" rel="noopener noreferrer" className="message-link">{displayUrl}</a>);
      }
      lastIndex = match.index + cleanUrl.length;
    }
    if (lastIndex < text.length) parts.push(text.slice(lastIndex));
    return parts;
  };

  useEffect(() => {
    fetchDialogs();
    telegramService.onDownloadProgress((data) => {
      setProgress({ total: data.total, downloaded: data.downloaded, currentFile: data.currentFile, topicTitle: data.topicTitle, isScanning: data.isScanning, items: data.items });
    });
    
    const unsubscribeBulk = telegramService.onSaveMultipleProgress((data) => {
      setBulkProgress(data);
      if (data.status === 'completed') {
        setTimeout(() => {
          setBulkDownloadActive(false);
          setBulkProgress(null);
        }, 3000);
      }
    });

    return () => {
      unsubscribeBulk();
    };
  }, []);

  useEffect(() => {
    const refreshFakeChats = () => {
      fetchDialogs();
    };
    window.addEventListener('plasma-twitter-fake-chat-created', refreshFakeChats);
    return () => window.removeEventListener('plasma-twitter-fake-chat-created', refreshFakeChats);
  }, []);

  useEffect(() => {
    if (showDetailedProgress && progressDetailsListRef.current) {
      progressDetailsListRef.current.scrollTop = progressDetailsListRef.current.scrollHeight;
    }
  }, [progress?.items, showDetailedProgress]);

  useEffect(() => { handleTelegramLinkRef.current = handleTelegramLink; });

  useEffect(() => {
    telegramService.onDeepLink((url) => { handleTelegramLinkRef.current?.(url); });
  }, []);

  useEffect(() => {
    if (selectedChat) {
      shouldScrollToBottomRef.current = true;
      setIsDownloadModalOpen(false);
      setForumTopics([]);
      setSelectedTopicId('all');
      setViewingTopic(null);
      setInputText('');
      setReplyTo(null);
      setSelectedFile(null);
      setSendProgress(null);
      setMsgContextMenu(null);
      setChatContextMenu(null);
      setIsSelectionMode(false);
      setSelectedMessageIds([]);
      setIsCreatingTopic(false);
      setNewTopicTitle('');
      setIsMenuOpen(false);
      setFullChatInfo(null);
      setSharedMedia([]);
      if (selectedChat.isInvite) {
        setFullChatInfo({
          about: selectedChat.about,
          participantsCount: selectedChat.participantsCount
        });
        setMessages([]);
        setHasMoreMessages(false);
      } else {
        setMessages([]);
        setHasMoreMessages(false);
        setOldestMessageId(null);
        fetchForumTopics(selectedChat);
        fetchFullChat(selectedChat.id);
        fetchSharedMedia(selectedChat.id);
        if (!selectedChat.hasTopics) loadMessages(selectedChat.id);
        else topicListScrollRef.current = 0;
      }
    } else {
      setMessages([]);
      setHasMoreMessages(false);
      setOldestMessageId(null);
      setForumTopics([]);
      setSelectedTopicId('all');
      setViewingTopic(null);
      setInputText('');
      setReplyTo(null);
      setSelectedFile(null);
      setMsgContextMenu(null);
      setIsSelectionMode(false);
      setSelectedMessageIds([]);
      setIsCreatingTopic(false);
      setNewTopicTitle('');
    }
  }, [selectedChat]);

  useEffect(() => {
    if (shouldScrollToBottomRef.current && virtuosoRef.current && messages.length > 0) {
      const lastIndex = Math.max(0, 100000 - messages.length) + messages.length - 1;
      virtuosoRef.current.scrollToIndex({ index: lastIndex, align: 'end' });
      const timer = setTimeout(() => {
        if (virtuosoRef.current) {
          virtuosoRef.current.scrollToIndex({ index: lastIndex, align: 'end', behavior: 'smooth' });
        }
      }, 100);
      shouldScrollToBottomRef.current = false;
      return () => clearTimeout(timer);
    }
  }, [messages]);

  useEffect(() => {
    messagesRef.current = messages;
  }, [messages]);

  useEffect(() => {
    if (pendingJumpToMsgIdRef.current) {
      const targetId = pendingJumpToMsgIdRef.current;
      const originalMsgIdx = messages.findIndex(m => Number(m.id) === Number(targetId));
      if (originalMsgIdx >= 0) {
        pendingJumpToMsgIdRef.current = null;
        const firstItemIndex = Math.max(0, 100000 - messages.length);
        const virtuosoIdx = firstItemIndex + originalMsgIdx;
        triggerJumpScroll(targetId, virtuosoIdx);
      }
    }
  }, [messages]);

  useEffect(() => {
    if (!viewingTopic && !loadingTopics && selectedChat?.hasTopics && topicListRef.current) {
      topicListRef.current.scrollTop = topicListScrollRef.current;
    }
  }, [viewingTopic, loadingTopics, selectedChat?.id]);

  useEffect(() => {
    const handleClickOutside = () => setIsTopicDropdownOpen(false);
    if (isTopicDropdownOpen) {
      document.addEventListener('click', handleClickOutside);
      return () => document.removeEventListener('click', handleClickOutside);
    }
  }, [isTopicDropdownOpen]);

  useEffect(() => {
    const handleClickOutside = () => setIsMenuOpen(false);
    if (isMenuOpen) {
      setTimeout(() => document.addEventListener('click', handleClickOutside), 0);
      return () => document.removeEventListener('click', handleClickOutside);
    }
  }, [isMenuOpen]);

  useEffect(() => {
    const handleClickOutside = () => setIsSettingsMenuOpen(false);
    if (isSettingsMenuOpen) {
      setTimeout(() => document.addEventListener('click', handleClickOutside), 0);
      return () => document.removeEventListener('click', handleClickOutside);
    }
  }, [isSettingsMenuOpen]);

  const fetchDialogs = async () => {
    try {
      const res = await telegramService.getDialogs();
      if (res.success && res.dialogs) {
        setChats(res.dialogs);
        const pendingFakeChatId = localStorage.getItem('plasma_twitter_pending_fake_chat');
        if (pendingFakeChatId) {
          const pendingChat = res.dialogs.find((chat: Chat) => chat.id === pendingFakeChatId);
          if (pendingChat) {
            setSelectedChat(pendingChat);
            localStorage.removeItem('plasma_twitter_pending_fake_chat');
          }
        }
        // Preload avatars in background so the list feels instant
        preloadAvatars(res.dialogs);
      }
      else setError(res.error || 'Failed to fetch chats');
    } catch (e: any) {
      setError(e.message || 'Unknown error');
    } finally {
      setLoading(false);
    }
  };

  const preloadAvatars = async (dialogs: Chat[]) => {
    const BATCH_SIZE = 8;
    const MAX_PRELOAD = 60;
    const targets = dialogs.slice(0, MAX_PRELOAD).filter(d => d.id && typeof d.id === 'string' && !d.id.startsWith('invite_'));
    for (let i = 0; i < targets.length; i += BATCH_SIZE) {
      const batch = targets.slice(i, i + BATCH_SIZE);
      await Promise.all(
        batch.map(d => telegramService.getAvatar(d.id).catch(() => null))
      );
      if (i + BATCH_SIZE < targets.length) {
        await new Promise(r => setTimeout(r, 80));
      }
    }
  };

  const fetchSharedMedia = async (chatId: string) => {
    setLoadingSharedMedia(true);
    try {
      const res = await telegramService.getSharedMedia({ chatId, limit: 12 });
      if (res.success) setSharedMedia(res.media);
    } catch (e) { console.error(e); }
    finally { setLoadingSharedMedia(false); }
  };

  const fetchForumTopics = async (chat: Chat) => {
    if (!chat.hasTopics) return;
    setLoadingTopics(true);
    try {
      const res = await telegramService.getForumTopics(chat.id);
      if (res.success && res.topics) setForumTopics(res.topics);
      else if (!res.success) setError(res.error || 'Failed to fetch topics');
    } catch (e: any) {
      setError(e.message || 'Unknown error');
    } finally {
      setLoadingTopics(false);
    }
  };

  const fetchFullChat = async (chatId: string) => {
    setLoadingFullInfo(true);
    try {
      const res = await telegramService.getFullChat(chatId);
      if (res.success && res.fullInfo) {
        setFullChatInfo(res.fullInfo);
      }
    } catch (e) { console.error(e); }
    finally { setLoadingFullInfo(false); }
  };

  const handleSelectTopic = (topic: ForumTopic) => {
    if (topicListRef.current) topicListScrollRef.current = topicListRef.current.scrollTop;
    setViewingTopic(topic);
    setSelectedTopicId(String(topic.id));
    loadMessages(selectedChat!.id, 0, topic.id);

    if (topic.unreadCount > 0) {
      telegramService.readHistory(selectedChat!.id).catch(console.error);
      setForumTopics(prev => prev.map(t => t.id === topic.id ? { ...t, unreadCount: 0 } : t));
      // Also update the chat's total unread count if needed, but usually it's better to let the server handle it on next fetch.
      // For now, let's just clear the topic count.
    }
  };

  const handleBackToTopics = () => {
    setViewingTopic(null);
    setSelectedTopicId('all');
    setMessages([]);
    setHasMoreMessages(false);
    setOldestMessageId(null);
  };

  const handleViewAllTopics = () => {
    if (topicListRef.current) topicListScrollRef.current = topicListRef.current.scrollTop;
    setViewingTopic({ id: 0, title: 'Todos os tópicos', topMessageId: 0, unreadCount: 0, closed: false, pinned: false });
    setSelectedTopicId('all');
    loadMessages(selectedChat!.id);
  };

  const loadMessages = async (chatId: string, offsetId = 0, topicId?: number, options: { silent?: boolean } = {}) => {
    if (!options.silent) setLoadingMessages(true);
    try {
      const res = await telegramService.getMessages({ chatId, limit: PAGE_SIZE, offsetId, topicId });
      if (res.success && res.messages) {
        setMessages(res.messages);
        setHasMoreMessages(Boolean(res.hasMore));
        setOldestMessageId(res.oldestMessageId ?? null);
      }
    } catch (e) { console.error(e); }
    finally { setLoadingMessages(false); }
  };

  const loadOlderMessages = async () => {
    if (!selectedChat || !oldestMessageId || loadingMoreMessages) return;
    setLoadingMoreMessages(true);
    if (timelineRef.current) preserveScrollPositionRef.current = timelineRef.current.scrollHeight;
    try {
      const res = await telegramService.getMessages({
        chatId: selectedChat.id, limit: PAGE_SIZE, offsetId: oldestMessageId, topicId: viewingTopic?.id
      });
      if (res.success && res.messages?.length) {
        setMessages(current => [...res.messages!, ...current]);
        setHasMoreMessages(Boolean(res.hasMore));
        setOldestMessageId(res.oldestMessageId ?? null);
      } else setHasMoreMessages(false);
    } catch (e) { console.error(e); }
    finally { setLoadingMoreMessages(false); }
  };

  const handleSelectFolder = async () => {
    const res = await telegramService.selectFolder();
    if (res.success && res.folderPath) setFolderPath(res.folderPath);
  };

  const handleStartDownload = async () => {
    if (!selectedChat || !folderPath) return;
    const selectedTopic = forumTopics.find(topic => String(topic.id) === selectedTopicId) || null;
    setDownloading(true); setStopping(false); setProgress(null);
    try {
      const res = await telegramService.startDownload({
        chatId: selectedChat.id, folderPath,
        topic: selectedTopic ? { id: selectedTopic.id, title: selectedTopic.title, topMessageId: selectedTopic.topMessageId } : null,
        splitByUser: !selectedChat.hasTopics ? splitByUser : false
      });
      if (!res.success) setError(res.error || 'Failed to start download');
    } catch (e: any) { setError(e.message || 'Unknown error'); }
    finally { setDownloading(false); setStopping(false); }
  };

  const handleStopDownload = async () => { setStopping(true); await telegramService.stopDownload(); };

  const triggerUserMediaSearch = async (userId: string) => {
    if (!selectedChat) return;
    setSearchMediaLoading(true);
    setSearchMediaResults([]);
    try {
      const res = await telegramService.searchUserMedia({
        chatId: selectedChat.id,
        userId,
        limit: 100
      });
      if (res.success && res.media) {
        setSearchMediaResults(res.media);
      } else {
        console.error('Failed to search user media:', res.error);
      }
    } catch (err) {
      console.error('Error during media search:', err);
    } finally {
      setSearchMediaLoading(false);
    }
  };

  const handleBulkDownload = async (messageIds: number[]) => {
    if (!selectedChat || !messageIds.length) return;

    const folderRes = await telegramService.selectFolder();
    if (!folderRes.success || !folderRes.folderPath) return;

    setBulkDownloadActive(true);
    setBulkProgress({
      total: messageIds.length,
      downloaded: 0,
      currentFile: 'Iniciando download...',
      status: 'started'
    });

    try {
      await telegramService.saveMultipleMediaFiles({
        chatId: selectedChat.id,
        messageIds,
        folderPath: folderRes.folderPath
      });
    } catch (err) {
      console.error('Error during bulk download:', err);
      setBulkDownloadActive(false);
      setBulkProgress(null);
    }
  };

  const handleStopBulkDownload = async () => {
    await telegramService.stopSaveMultiple();
    setBulkDownloadActive(false);
    setBulkProgress(null);
  };

  const handleSelectFile = async () => {
    const res = await telegramService.selectFile();
    if (res.success && res.filePath) setSelectedFile({ filePath: res.filePath, fileName: res.fileName! });
  };

  const handleSend = async () => {
    if (!selectedChat || (!inputText.trim() && !selectedFile) || isSending) return;
    const topicId = viewingTopic && viewingTopic.id !== 0 ? viewingTopic.id : undefined;
    const replyToId = replyTo?.id;
    const textToSend = inputText.trim();
    const fileToSend = selectedFile;

    // Clear input immediately for better UX
    setInputText('');
    setReplyTo(null);
    setSelectedFile(null);
    setIsSending(true);

    if (fileToSend) setSendProgress(0);
    try {
      let res;
      if (fileToSend) {
        const unsub = telegramService.onSendProgress((data) => setSendProgress(data.progress));
        try {
          res = await telegramService.sendMedia({
            chatId: selectedChat.id,
            filePath: fileToSend.filePath,
            caption: textToSend || undefined,
            replyToId,
            topicId
          });
        } finally { unsub(); }
      } else {
        res = await telegramService.sendMessage({
          chatId: selectedChat.id,
          text: textToSend,
          replyToId,
          topicId
        });
      }

      if (res.success) {
        shouldScrollToBottomRef.current = true;
        // Refresh messages silently in background
        loadMessages(selectedChat.id, 0, topicId, { silent: true });
      } else {
        setError(res.error || 'Falha ao enviar');
        // Restore input text on error so user doesn't lose it
        setInputText(textToSend);
        if (fileToSend) setSelectedFile(fileToSend);
      }
    } catch (e: any) {
      setError(e.message || 'Erro ao enviar');
      setInputText(textToSend);
      if (fileToSend) setSelectedFile(fileToSend);
    } finally {
      setIsSending(false);
      setSendProgress(null);
    }
  };

  const handleCreateTopic = async () => {
    if (!selectedChat || !newTopicTitle.trim()) return;
    try {
      const res = await telegramService.createTopic({ chatId: selectedChat.id, title: newTopicTitle.trim(), iconColor: newTopicColor });
      if (res.success) { setIsCreatingTopic(false); setNewTopicTitle(''); setNewTopicColor(7322096); fetchForumTopics(selectedChat); }
      else setError(res.error || 'Falha ao criar tópico');
    } catch (e: any) { setError(e.message || 'Erro ao criar tópico'); }
  };



  const handleViewOriginalMessage = async (msg: Message) => {
    if (!selectedChat) return;
    const res = await telegramService.getOriginalMessage({ chatId: selectedChat.id, messageId: msg.id });
    if (res.success && res.message) {
      setOriginalMsgModal({ msg, original: res.message });
      setMsgContextMenu(null);
    }
  };

  const handleReact = useCallback(async (msg: Message, emoji: string) => {
    setEmojiPickerMsgId(null);
    if (!selectedChat) return;

    // Optimistic UI Update
    setMessages(prev => prev.map(m => {
      if (m.id !== msg.id) return m;
      
      const reactions = [...(m.reactions || [])];
      const existingIdx = reactions.findIndex(r => r.emoji === emoji);
      
      if (existingIdx > -1) {
        const r = reactions[existingIdx];
        if (r.mine) {
          // Remove my reaction
          if (r.count <= 1) reactions.splice(existingIdx, 1);
          else reactions[existingIdx] = { ...r, count: r.count - 1, mine: false };
        } else {
          // Toggle to mine
          reactions[existingIdx] = { ...r, count: r.count + 1, mine: true };
        }
      } else {
        // Add new reaction
        reactions.push({ emoji, count: 1, mine: true });
      }
      
      return { ...m, reactions };
    }));

    try {
      await telegramService.sendReaction({ chatId: selectedChat.id, messageId: msg.id, reaction: emoji });
    } catch (err) {
      console.error('Failed to send reaction:', err);
      // Revert or fetch messages again if needed
    }
  }, [selectedChat]);

  useEffect(() => {
    if (emojiPickerMsgId === null) return;
    const close = () => setEmojiPickerMsgId(null);
    document.addEventListener('click', close);
    return () => document.removeEventListener('click', close);
  }, [emojiPickerMsgId]);

  const triggerJumpScroll = (targetMsgId: number, virtuosoIdx: number) => {
    console.log('[TelegramEnchanted] Triggering jump scroll to index:', virtuosoIdx, 'for msg ID:', targetMsgId);
    if (virtuosoRef.current) {
      virtuosoRef.current.scrollToIndex({ index: virtuosoIdx, align: 'center' });
    }

    const targetId = `msg-${targetMsgId}`;
    let attemptsCount = 0;
    const pollAndAlign = () => {
      const domEl = document.getElementById(targetId);
      if (domEl) {
        console.log('[TelegramEnchanted] Found target in DOM. Scrolling into center view.');
        domEl.scrollIntoView({ behavior: 'smooth', block: 'center' });
        
        // Highlight flash effect
        setHighlightedMsgId(targetMsgId);
        setTimeout(() => setHighlightedMsgId(null), 1500);
        return true;
      }
      return false;
    };

    // Try instantly first
    if (!pollAndAlign()) {
      const intervalId = setInterval(() => {
        attemptsCount++;
        const success = pollAndAlign();
        if (success || attemptsCount > 60) { // 60 * 50ms = 3000ms max polling
          clearInterval(intervalId);
          if (!success) {
            console.warn('[TelegramEnchanted] Polling failed to find target in DOM after 3 seconds.');
          }
        }
      }, 50);
    }
  };

  const handleJumpToMessage = async (replyToMsgId: number) => {
    let currentMessages = [...messagesRef.current];
    console.log('[TelegramEnchanted] Jump to Message triggered. Target replyToMsgId:', replyToMsgId);
    console.log('[TelegramEnchanted] Total loaded messages:', currentMessages.length);

    let originalMsgIdx = currentMessages.findIndex(m => Number(m.id) === Number(replyToMsgId));
    console.log('[TelegramEnchanted] Message index in array:', originalMsgIdx);

    if (originalMsgIdx >= 0) {
      // Message already in memory, scroll to it immediately
      const firstItemIndex = Math.max(0, 100000 - currentMessages.length);
      const virtuosoIdx = firstItemIndex + originalMsgIdx;
      triggerJumpScroll(replyToMsgId, virtuosoIdx);
    } else {
      // 1. If not found in memory, load older messages automatically (up to 15 attempts / 750 messages)
      if (hasMoreMessages && oldestMessageId) {
        pendingJumpToMsgIdRef.current = replyToMsgId;
        setLoadingMessages(true);
        try {
          let currentOldestId: number | null = oldestMessageId;
          let found = false;
          let attempts = 0;
          let newMessages = [...currentMessages];
          
          while (!found && currentOldestId && attempts < 15) {
            console.log('[TelegramEnchanted] Target message not found in local feed. Loading older chunk... Attempt:', attempts + 1);
            const res = await telegramService.getMessages({
              chatId: selectedChat!.id,
              limit: PAGE_SIZE,
              offsetId: currentOldestId,
              topicId: viewingTopic?.id
            });
            
            if (res.success && res.messages?.length) {
              newMessages = [...res.messages, ...newMessages];
              currentOldestId = res.oldestMessageId ?? null;
              
              originalMsgIdx = newMessages.findIndex(m => Number(m.id) === Number(replyToMsgId));
              if (originalMsgIdx >= 0) {
                found = true;
                setMessages(newMessages);
                setHasMoreMessages(Boolean(res.hasMore));
                setOldestMessageId(res.oldestMessageId ?? null);
                break;
              }
              if (!res.hasMore) {
                break;
              }
            } else {
              break;
            }
            attempts++;
          }
          
          if (!found) {
            pendingJumpToMsgIdRef.current = null;
            setError('A mensagem original não foi encontrada no histórico.');
          }
        } catch (e) {
          console.error('Error loading older messages for jump:', e);
          pendingJumpToMsgIdRef.current = null;
        } finally {
          setLoadingMessages(false);
        }
      } else {
        console.warn('[TelegramEnchanted] Target message not found in local feed and cannot load older.');
        setError('A mensagem original está muito antiga.');
      }
    }
  };

  const showComposer = !selectedChat?.hasTopics || (viewingTopic && viewingTopic.id !== 0);

  if (loading) return (
    <div className="full-screen-loader fade-in">
      <div className="loader-content">
        <div className="spinner large-spinner" />
        <p>Carregando conversas...</p>
      </div>
    </div>
  );

  return (
    <>
      <div className="app" data-palette={palette} data-density={density}>

        {/* ── List ─────────────────────────────────────────────────── */}
        <div className="list">
          <div className="list-header">
            <div className="list-title">
              <h1>Chats</h1>
              <div className="list-title-actions">
                <button
                  className={`icon-btn ${isSearchOpen ? 'active' : ''}`}
                  onClick={() => { setIsSearchOpen(v => !v); if (isSearchOpen) setChatSearch(''); }}
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
                  onChange={e => setChatSearch(e.target.value)}
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
              <span className="count">{chats.filter(c => (c.unreadCount ?? 0) > 0).length}</span>
            </div>
          </div>

          {error && (
            <div style={{ padding: '8px 12px', fontSize: 12, color: 'var(--danger)', background: 'color-mix(in oklch, var(--danger) 10%, var(--bg-1))' }}>
              {error}
            </div>
          )}

          <div className="chats">
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
                      telegramService.readHistory(chat.id).catch(console.error);
                      setChats(prev => prev.map(c => c.id === chat.id ? { ...c, unreadCount: 0 } : c));
                    }
                  }}
                  onContextMenu={(e) => {
                    e.preventDefault();
                    setChatContextMenu({ x: e.clientX, y: e.clientY, chat });
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
            {!filteredChats.length && !loading && (
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
                onClick={e => { e.stopPropagation(); setIsSettingsMenuOpen(v => !v); }}
                title="Configurações e Aparência"
              >
                <IconSettings />
              </button>
              {isSettingsMenuOpen && (
                <div className="dropdown-menu" style={{ bottom: 'calc(100% + 8px)', top: 'auto', right: 0 }} onClick={e => e.stopPropagation()}>
                  <div className="dropdown-item" onClick={() => { setIsSettingsOpen(true); setIsSettingsMenuOpen(false); }}>
                    <IconSettings /> Configurações de Cache
                  </div>
                  <div className="dropdown-divider" />
                  <div style={{ padding: '6px 12px 4px', fontSize: 11, color: 'var(--text-3)', fontWeight: 600, letterSpacing: '0.05em', textTransform: 'uppercase' }}>Paleta</div>
                  <div className="palette-picker">
                    {PALETTES.map(p => (
                      <button key={p} className={`palette-dot ${p} ${palette === p ? 'active' : ''}`} onClick={() => setPalette(p)} title={p} />
                    ))}
                  </div>
                  <div style={{ padding: '2px 12px 4px', fontSize: 11, color: 'var(--text-3)', fontWeight: 600, letterSpacing: '0.05em', textTransform: 'uppercase' }}>Densidade</div>
                  <div className="density-row">
                    {DENSITIES.map(d => (
                      <button key={d} className={`density-btn ${density === d ? 'active' : ''}`} onClick={() => setDensity(d)}>{d}</button>
                    ))}
                  </div>
                </div>
              )}
            </div>
          </div>
        </div>

        {/* ── Convo ────────────────────────────────────────────────── */}
        <div 
          className={`convo ${isDraggingOver ? 'dragging-over' : ''} ${isSelectionMode ? 'is-selection-mode' : ''}`}
          onDragOver={(e) => { e.preventDefault(); setIsDraggingOver(true); }}
          onDragLeave={(e) => { e.preventDefault(); setIsDraggingOver(false); }}
          onDrop={(e) => {
            e.preventDefault();
            setIsDraggingOver(false);
            if (e.dataTransfer.files && e.dataTransfer.files.length > 0) {
              const file = e.dataTransfer.files[0] as File & { path?: string };
              const realPath = telegramService.getPathForFile ? telegramService.getPathForFile(file) : file.path;
              if (realPath) {
                setSelectedFile({ filePath: realPath, fileName: file.name });
              } else {
                setError('O arquivo precisa ser arrastado de uma pasta do seu computador.');
              }
            }
          }}
        >
          {selectedChat ? (
            <>
              {/* Header */}
              <div className="convo-header">
                <div className="who">
                  {selectedChat.hasTopics && viewingTopic && (
                    <button className="icon-btn" onClick={handleBackToTopics} title="Voltar para tópicos">
                      <IconBack />
                    </button>
                  )}
                  <div className={`who-avatar chat-avatar color-${hashColor(selectedChat.id)}`}>
                    <ChatAvatar chatId={selectedChat.id} title={selectedChat.title} />
                  </div>
                  <div className="who-text">
                    <div className="who-name">
                      {viewingTopic ? viewingTopic.title : selectedChat.title}
                    </div>
                    <div className="who-sub">
                      {viewingTopic
                        ? `${selectedChat.title} · ${getChatKind(selectedChat)}`
                        : `${getChatKind(selectedChat)}${selectedChat.hasTopics ? ' · fórum' : ''} · ${messages.length} mensagens`}
                    </div>
                  </div>
                </div>
                <div className="convo-header-actions">
                  {selectedChat.isMember === false && (
                    <button
                      className="join-btn-header"
                      onClick={async () => {
                        const res = await telegramService.joinChat(selectedChat.id);
                        if (res.success) {
                          setSelectedChat(prev => prev ? { ...prev, isMember: true } : null);
                          fetchDialogs();
                          setConfirmModal({
                            title: 'Sucesso',
                            body: res.message || 'Você entrou no grupo com sucesso!',
                            onConfirm: () => setConfirmModal(null)
                          });
                        } else {
                          setConfirmModal({
                            title: 'Erro',
                            body: `Erro ao entrar: ${res.error}`,
                            onConfirm: () => setConfirmModal(null)
                          });
                        }
                      }}
                    >
                      Entrar no {getChatKind(selectedChat)}
                    </button>
                  )}
                  {selectedChat.hasTopics && !viewingTopic && (
                    <button
                      className={`icon-btn ${isCreatingTopic ? 'active' : ''}`}
                      onClick={() => { setIsCreatingTopic(v => !v); setNewTopicTitle(''); }}
                      title="Criar tópico"
                    >
                      <IconPlus />
                    </button>
                  )}
                  <button
                    className={`icon-btn ${infoOpen ? 'active' : ''}`}
                    onClick={() => setInfoOpen(v => !v)}
                    title="Painel de informações"
                  >
                    <IconPanel />
                  </button>
                  <div style={{ position: 'relative' }}>
                    <button className="icon-btn" onClick={e => { e.stopPropagation(); setIsMenuOpen(v => !v); }} title="Mais opções">
                      <IconMore />
                    </button>
                    {isMenuOpen && (
                      <div className="dropdown-menu" onClick={e => e.stopPropagation()}>
                        <div className="dropdown-item" onClick={() => { setIsDownloadModalOpen(v => !v); setIsMenuOpen(false); }}>
                          <IconMagic />
                          <span>Mass Download</span>
                        </div>
                        <div className="dropdown-item" onClick={() => { setIsSelectionMode(true); setIsMenuOpen(false); setSelectedMessageIds([]); }}>
                          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
                            <path d="M22 11.08V12a10 10 0 1 1-5.93-9.14" />
                            <polyline points="22 4 12 14.01 9 11.01" />
                          </svg>
                          <span>Selecionar mídias</span>
                        </div>
                      </div>
                    )}
                  </div>
                </div>
              </div>

              {/* Panels */}
              <div className="convo-panels">
                {isDownloadModalOpen && (
                  <div className="inline-download-panel">
                    <div className="inline-panel-header">
                      <div className="inline-panel-title">
                        <IconMagic />
                        <h3>Mass Download</h3>
                      </div>
                      <button className="icon-btn" onClick={() => setIsDownloadModalOpen(false)}>✕</button>
                    </div>
                    <div className="inline-panel-body">
                      <div className="inline-folder">
                        <div className="folder-selection">
                          <input readOnly value={folderPath} placeholder="Selecionar pasta de destino..." />
                          <button className="browse-btn" onClick={handleSelectFolder}>Procurar</button>
                        </div>
                      </div>
                      {!selectedChat.hasTopics && (
                        <div className="split-user-selection">
                          <label className="switch-label">
                            <input
                              type="checkbox"
                              checked={splitByUser}
                              onChange={(e) => setSplitByUser(e.target.checked)}
                              disabled={downloading}
                            />
                            <span className="switch-custom" />
                            <span className="switch-text">Dividir mídias por usuário</span>
                          </label>
                        </div>
                      )}
                      {selectedChat.hasTopics && (
                        <div className="topic-selection">
                          <div className={`custom-select ${isTopicDropdownOpen ? 'open' : ''} ${loadingTopics || downloading ? 'disabled' : ''}`}>
                            <button
                              type="button"
                              className="custom-select-trigger"
                              onClick={e => { e.stopPropagation(); if (!loadingTopics && !downloading) setIsTopicDropdownOpen(v => !v); }}
                              disabled={loadingTopics || downloading}
                            >
                              <span>
                                {loadingTopics ? 'Carregando...' : selectedTopicId === 'all' ? 'Todos os tópicos' : forumTopics.find(t => String(t.id) === selectedTopicId)?.title || 'Todos os tópicos'}
                              </span>
                              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round"><polyline points="6 9 12 15 18 9" /></svg>
                            </button>
                            {isTopicDropdownOpen && (
                              <div className="custom-select-options">
                                <div className="custom-select-search" onClick={e => e.stopPropagation()}>
                                  <input type="text" placeholder="Pesquisar tópicos..." value={topicSearch} onChange={e => setTopicSearch(e.target.value)} autoFocus />
                                </div>
                                <button type="button" className={`custom-select-option ${selectedTopicId === 'all' ? 'selected' : ''}`} onClick={e => { e.stopPropagation(); setSelectedTopicId('all'); setIsTopicDropdownOpen(false); setTopicSearch(''); }}>
                                  Todos os tópicos
                                </button>
                                {filteredTopics.map(topic => (
                                  <button key={topic.id} type="button" className={`custom-select-option ${String(topic.id) === selectedTopicId ? 'selected' : ''}`} onClick={e => { e.stopPropagation(); setSelectedTopicId(String(topic.id)); setIsTopicDropdownOpen(false); setTopicSearch(''); }}>
                                    {topic.pinned && <span className="option-pin">📌</span>}
                                    {topic.title}
                                  </button>
                                ))}
                              </div>
                            )}
                          </div>
                        </div>
                      )}
                      {downloading ? (
                        <button className={`stop-btn ${stopping ? 'disabled' : ''}`} onClick={handleStopDownload} disabled={stopping}>
                          {stopping ? '⏳ Parando...' : '⏹ Parar'}
                        </button>
                      ) : (
                        <button className="start-btn" onClick={handleStartDownload} disabled={!folderPath || loadingTopics}>
                          Iniciar
                        </button>
                      )}
                    </div>
                    {progress && (
                      <div 
                        className={`inline-progress-container ${showDetailedProgress ? 'expanded' : ''}`}
                        onClick={() => setShowDetailedProgress(!showDetailedProgress)}
                        style={{ cursor: 'pointer', userSelect: 'none' }}
                      >
                        <div className="progress-header">
                          <span className="progress-status">
                            {progress.topicTitle ? `${progress.topicTitle} · ${progress.currentFile}` : progress.currentFile}
                          </span>
                          <span className="progress-count" style={{ display: 'flex', alignItems: 'center', gap: '4px' }}>
                            {progress.isScanning ? 'Escaneando...' : `${Math.floor(progress.downloaded)} / ${progress.total}`}
                            <span style={{ fontSize: '10px', transition: 'transform 0.2s', transform: showDetailedProgress ? 'rotate(180deg)' : 'rotate(0deg)' }}>
                              ▼
                            </span>
                          </span>
                        </div>
                        <div className="progress-bar">
                          <div
                            className={`progress-fill ${downloading ? 'animated-stripes' : ''} ${progress.isScanning ? 'scanning-fill' : ''}`}
                            style={{ width: (progress.total > 0 && !progress.isScanning) ? `${Math.min(100, (progress.downloaded / progress.total) * 100)}%` : '100%' }}
                          />
                        </div>
                        {showDetailedProgress && progress.items && progress.items.length > 0 && (
                          <div ref={progressDetailsListRef} className="progress-details-list" onClick={e => e.stopPropagation()}>
                            {progress.items.map((item, idx) => (
                              <div key={idx} className={`progress-item-row ${item.status}`}>
                                <div className="progress-item-info">
                                  <span className="progress-item-name" title={item.name}>{item.name}</span>
                                  {item.size > 0 && (
                                    <span className="progress-item-size">
                                      {formatBytes(item.size)}
                                    </span>
                                  )}
                                </div>
                                <div className="progress-item-status-col">
                                  {item.status === 'downloading' && (
                                    <span className="item-badge downloading">
                                      <span className="spinner small-spinner inline-spinner" style={{ width: 10, height: 10, borderWidth: 1.5, display: 'inline-block', marginRight: 4 }} />
                                      {item.progress}%
                                    </span>
                                  )}
                                  {item.status === 'completed' && (
                                    <span className="item-badge completed">✓ Salvo</span>
                                  )}
                                  {item.status === 'skipped' && (
                                    <span className="item-badge skipped">⌥ Já existe</span>
                                  )}
                                  {item.status === 'failed' && (
                                    <span className="item-badge failed">✕ Falhou</span>
                                  )}
                                </div>
                              </div>
                            ))}
                          </div>
                        )}
                      </div>
                    )}
                  </div>
                )}

                {isCreatingTopic && selectedChat.hasTopics && !viewingTopic && (
                  <div className="new-topic-form">
                    <div className="new-topic-header">
                      <span className="new-topic-label">Novo Tópico</span>
                      <button type="button" className="icon-btn" onClick={() => { setIsCreatingTopic(false); setNewTopicTitle(''); }}>✕</button>
                    </div>
                    <div className="new-topic-body">
                      <input
                        type="text"
                        className="new-topic-input"
                        value={newTopicTitle}
                        onChange={e => setNewTopicTitle(e.target.value.slice(0, 128))}
                        placeholder="Nome do tópico..."
                        maxLength={128}
                        autoFocus
                        onKeyDown={e => {
                          if (e.key === 'Enter') handleCreateTopic();
                          if (e.key === 'Escape') { setIsCreatingTopic(false); setNewTopicTitle(''); }
                        }}
                      />
                      <div className="topic-color-picker">
                        {TOPIC_ICON_COLORS.map(color => (
                          <button key={color.value} type="button" className={`topic-color-dot ${newTopicColor === color.value ? 'selected' : ''}`} style={{ background: color.css }} onClick={() => setNewTopicColor(color.value)} title={color.label} />
                        ))}
                      </div>
                      <button type="button" className="new-topic-create" onClick={handleCreateTopic} disabled={!newTopicTitle.trim()}>
                        Criar
                      </button>
                    </div>
                  </div>
                )}
              </div>

              {/* Content */}
              {selectedChat.hasTopics && !viewingTopic ? (
                /* Topic list */
                <div className="topic-list-panel" ref={topicListRef} onClick={() => isMenuOpen && setIsMenuOpen(false)}>
                  {loadingTopics ? (
                    <div className="messages-loading"><span className="spinner" /> Carregando tópicos...</div>
                  ) : forumTopics.length === 0 ? (
                    <div className="messages-empty">Nenhum tópico encontrado.</div>
                  ) : (
                    <>
                      <div className="topic-search-row">
                        <input type="text" placeholder="Pesquisar tópicos..." value={topicSearch} onChange={e => setTopicSearch(e.target.value)} />
                      </div>
                      <div className="topic-item" onClick={handleViewAllTopics}>
                        <div className="topic-item-avatar topic-item-avatar-all">
                          <svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                            <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" />
                          </svg>
                        </div>
                        <div className="topic-item-content">
                          <div className="topic-item-name">Todos os tópicos</div>
                          <div className="topic-item-sub">Ver mensagens de todos os tópicos</div>
                        </div>
                      </div>
                      <div className="topic-divider" />
                      {filteredTopics.map(topic => (
                        <div key={topic.id} className="topic-item" onClick={() => handleSelectTopic(topic)}>
                          <div className="topic-item-avatar" style={{ background: `linear-gradient(135deg, ${getTopicColor(topic.id)})` }}>
                            {topic.title ? topic.title.charAt(0).toUpperCase() : '?'}
                          </div>
                          <div className="topic-item-content">
                            <div className="topic-item-name">
                              {topic.title}
                              {topic.pinned && <span className="topic-pin">📌</span>}
                            </div>
                            <div className="topic-item-sub">
                              {topic.unreadCount > 0 ? `${topic.unreadCount} não lidas${topic.closed ? ' · Fechado' : ''}` : topic.closed ? 'Fechado' : 'Aberto'}
                            </div>
                          </div>
                        </div>
                      ))}
                    </>
                  )}
                </div>
              ) : (
                /* Message timeline */
                <div
                  className="timeline"
                  ref={timelineRef}
                  style={{ overflowY: 'hidden' }}
                  onClick={() => { if (isMenuOpen) setIsMenuOpen(false); if (msgContextMenu) setMsgContextMenu(null); }}
                >
                  <div className="timeline-bg" />
                  <div className="timeline-bg-overlay" />
                  {loadingMessages && messages.length === 0 ? (
                    <div className="messages-loading" style={{ zIndex: 1 }}><span className="spinner" /> Carregando...</div>
                  ) : messages.length === 0 ? (
                    <div className="messages-empty" style={{ zIndex: 1 }}>Nenhuma mensagem encontrada.</div>
                  ) : (
                    <Virtuoso
                      ref={virtuosoRef}
                      style={{ height: '100%', width: '100%', outline: 'none', zIndex: 1 }}
                      data={getTimelineItems(messages)}
                      firstItemIndex={Math.max(0, 100000 - getTimelineItems(messages).length)}
                      startReached={loadOlderMessages}
                      components={{
                        List: ListContainer,
                        Header: () => {
                          if (!hasMoreMessages) return null;
                          return (
                            <div className="messages-load-more" style={{ display: 'flex', justifyContent: 'center', padding: '16px 0' }}>
                              {loadingMoreMessages ? (
                                <div className="messages-loading-inline" style={{ display: 'flex', alignItems: 'center', gap: '8px', fontSize: '13px', color: 'var(--text-3)' }}>
                                  <span className="spinner small-spinner" /> Carregando mensagens anteriores...
                                </div>
                              ) : (
                                <div style={{ height: '24px' }} />
                              )}
                            </div>
                          );
                        }
                      }}
                      itemContent={(index, item) => {
                        const timelineItems = getTimelineItems(messages);
                        const firstItemIndex = Math.max(0, 100000 - timelineItems.length);
                        const dataIndex = index - firstItemIndex;
                        const prevItem = timelineItems[dataIndex - 1];
                        const prev = prevItem?.message;
                        const msg = item.message;
                        const continued = isContinued(msg, prev);
                        const dayBreak = isNewMessageDay(msg, prev);
                        const color = msg.out ? 'cyan' : hashColor(msg.senderId || msg.id.toString());
                        const displayName = msg.out ? 'Você' : (msg.senderName || (msg.senderId ? `ID ${msg.senderId.slice(-6)}` : 'Desconhecido'));
                        const initials = msg.out ? 'EU' : displayName.slice(0, 2).toUpperCase();

                        return (
                          <div key={item.id} style={{ paddingBottom: 'var(--msg-gap)' }}>
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
                              onClick={() => setEmojiPickerMsgId(null)}
                              onContextMenu={(e) => { e.preventDefault(); e.stopPropagation(); setMsgContextMenu({ x: e.clientX, y: e.clientY, message: msg }); }}
                            >
                              {!msg.out && (
                                <div
                                  className={`msg-avatar color-${color}`}
                                  style={continued ? { visibility: 'hidden' } : undefined}
                                  onContextMenu={(e) => {
                                    if (msg.senderId) {
                                      e.preventDefault();
                                      e.stopPropagation();
                                      setUserContextMenu({
                                        x: e.clientX,
                                        y: e.clientY,
                                        senderId: msg.senderId,
                                        senderName: displayName
                                      });
                                    }
                                  }}
                                >
                                  {initials}
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
                                          setUserContextMenu({
                                            x: e.clientX,
                                            y: e.clientY,
                                            senderId: msg.senderId,
                                            senderName: displayName
                                          });
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
                                <div className={`msg-bubble ${!msg.text && (msg.hasMedia || item.type === 'album') ? 'media-only' : ''} ${highlightedMsgId === msg.id ? 'highlight-flash' : ''}`}>
                                  {msg.replyToMsgId && (() => {
                                    const repliedMsg = messages.find(m => Number(m.id) === Number(msg.replyToMsgId));
                                    const sender = repliedMsg
                                      ? (repliedMsg.out ? 'Você' : (repliedMsg.senderName || 'Desconhecido'))
                                      : `Mensagem #${msg.replyToMsgId}`;
                                    const text = repliedMsg
                                      ? (repliedMsg.text ? repliedMsg.text.slice(0, 60) : (repliedMsg.hasMedia ? '📷 Mídia' : ''))
                                      : 'Clique para saltar para a mensagem';

                                    return (
                                      <div
                                        className="msg-reply-preview"
                                        onClick={(e) => {
                                          e.stopPropagation();
                                          handleJumpToMessage(msg.replyToMsgId!);
                                        }}
                                      >
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
                                          className={`album-grid-item msg-media ${selectedMessageIds.includes(albumMsg.id) ? 'media-selected' : ''}`}
                                          onContextMenu={(e) => {
                                            e.preventDefault();
                                            e.stopPropagation();
                                            setImgContextMenu({ x: e.clientX, y: e.clientY, msg: albumMsg });
                                          }}
                                        >
                                          <MessageMedia
                                            chatId={selectedChat.id}
                                            messageId={albumMsg.id}
                                            isVideo={albumMsg.isVideo}
                                            videoDuration={albumMsg.videoDuration}
                                            messageDate={albumMsg.date}
                                            mediaSize={albumMsg.mediaSize}
                                            palette={palette}
                                            density={density}
                                            onClickOverride={isSelectionMode ? () => {
                                              setSelectedMessageIds(prevIds =>
                                                prevIds.includes(albumMsg.id)
                                                  ? prevIds.filter(id => id !== albumMsg.id)
                                                  : [...prevIds, albumMsg.id]
                                              );
                                            } : undefined}
                                            albumMedias={item.messages!.map(m => ({ id: m.id, isVideo: m.isVideo, videoDuration: m.videoDuration, messageDate: m.date, mediaSize: m.mediaSize }))}
                                          />
                                        </div>
                                      ))}
                                    </div>
                                  ) : (
                                    msg.hasMedia && (
                                      <div
                                        className={`msg-media ${selectedMessageIds.includes(msg.id) ? 'media-selected' : ''}`}
                                        onContextMenu={(e) => {
                                          e.preventDefault();
                                          e.stopPropagation();
                                          setImgContextMenu({ x: e.clientX, y: e.clientY, msg });
                                        }}
                                      >
                                        <MessageMedia
                                          chatId={selectedChat.id}
                                          messageId={msg.id}
                                          isVideo={msg.isVideo}
                                          videoDuration={msg.videoDuration}
                                          messageDate={msg.date}
                                          mediaSize={msg.mediaSize}
                                          palette={palette}
                                          density={density}
                                          onClickOverride={isSelectionMode ? () => {
                                            setSelectedMessageIds(prevIds =>
                                              prevIds.includes(msg.id)
                                                ? prevIds.filter(id => id !== msg.id)
                                                : [...prevIds, msg.id]
                                            );
                                          } : undefined}
                                        />
                                      </div>
                                    )
                                  )}
                                  {msg.text && <div className="msg-text">{linkifyText(msg.text)}</div>}
                                  {msg.isDeleted && (
                                    <div className="msg-deleted-badge">🗑️ Mensagem excluída no servidor</div>
                                  )}
                                </div>
                                {msg.reactions && msg.reactions.length > 0 && (
                                  <div className="reactions">
                                    {msg.reactions.map((r, i) => (
                                      <button key={i} className={`reaction ${r.mine ? 'mine' : ''}`} onClick={() => handleReact(msg, r.emoji)}>
                                        <span>{r.emoji}</span>
                                        <span>{r.count}</span>
                                      </button>
                                    ))}
                                    <button
                                      className="reaction-add"
                                      title="Reagir"
                                      onClick={(e) => {
                                        e.stopPropagation();
                                        const rect = (e.currentTarget as HTMLButtonElement).getBoundingClientRect();
                                        const pickerHeight = 300;
                                        const y = (rect.bottom + pickerHeight > window.innerHeight) 
                                          ? rect.top - pickerHeight - 8 
                                          : rect.bottom + 8;
                                        setEmojiPickerPos({ x: rect.left, y });
                                        setEmojiPickerMsgId(prev => prev === msg.id ? null : msg.id);
                                      }}
                                    >+</button>
                                  </div>
                                )}
                              </div>
                              <div className="msg-actions">
                                <button
                                  type="button" className="icon-btn" title="Reagir"
                                  onClick={(e) => {
                                    e.stopPropagation();
                                    const rect = (e.currentTarget as HTMLButtonElement).getBoundingClientRect();
                                    const pickerHeight = 300; // Approximate height of 21 emojis in 3 cols
                                    const y = (rect.bottom + pickerHeight > window.innerHeight) 
                                      ? rect.top - pickerHeight - 8 
                                      : rect.bottom + 8;
                                    setEmojiPickerPos({ x: rect.left, y });
                                    setEmojiPickerMsgId(prev => prev === msg.id ? null : msg.id);
                                  }}
                                >😊</button>
                                <button
                                  type="button" className="icon-btn" title="Responder"
                                  onClick={(e) => { e.stopPropagation(); setReplyTo(msg); }}
                                >↩</button>
                                <button
                                  type="button" className="icon-btn" title="Encaminhar"
                                  onClick={(e) => { e.stopPropagation(); /* TODO: forward */ }}
                                >→</button>
                                {item.type === 'album' ? (
                                  <button
                                    type="button" className="icon-btn" title="Salvar todas as mídias"
                                    onClick={async (e) => {
                                      e.stopPropagation();
                                      if (item.messages) {
                                        for (const albumMsg of item.messages) {
                                          try {
                                            await telegramService.saveMessageMediaFile({ chatId: selectedChat.id, messageId: albumMsg.id });
                                          } catch (err) {
                                            console.error(err);
                                          }
                                        }
                                      }
                                    }}
                                  >⤓</button>
                                ) : msg.hasMedia ? (
                                  <button
                                    type="button" className="icon-btn" title="Salvar"
                                    onClick={(e) => { e.stopPropagation(); telegramService.saveMessageMediaFile({ chatId: selectedChat.id, messageId: msg.id }); }}
                                  >⤓</button>
                                ) : msg.text ? (
                                  <button
                                    type="button" className="icon-btn" title="Copiar texto"
                                    onClick={(e) => { e.stopPropagation(); navigator.clipboard.writeText(msg.text); }}
                                  >⎘</button>
                                ) : null}
                              </div>
                            </div>
                          </div>
                        );
                      }}
                    />
                  )}
                </div>
              )}
              {/* Composer or Join Bar or Selection Bar */}
              {isSelectionMode ? (
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
                          <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/>
                          <polyline points="7 10 12 15 17 10"/>
                          <line x1="12" x2="12" y1="15" y2="3"/>
                        </svg>
                        <span>Baixar Selecionadas</span>
                      </button>
                    </div>
                  </div>
                </div>
              ) : (
                showComposer && (
                  selectedChat.isMember !== false ? (
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
                            onChange={e => setInputText(e.target.value)}
                            onInput={e => {
                              const ta = e.currentTarget;
                              ta.style.height = 'auto';
                              ta.style.height = `${Math.min(ta.scrollHeight, 160)}px`;
                            }}
                            onKeyDown={e => {
                              if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); handleSend(); }
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
                  ) : (
                    <div className="join-channel-bar">
                      <div className="join-channel-info">
                        <h3>{selectedChat.title}</h3>
                        {fullChatInfo?.participantsCount !== undefined && (
                          <span>{fullChatInfo.participantsCount.toLocaleString()} participantes</span>
                        )}
                      </div>
                      <button
                        className="join-channel-btn"
                        onClick={async () => {
                          const res = await telegramService.joinChat(selectedChat.isInvite ? `https://t.me/+${selectedChat.inviteHash}` : selectedChat.id);
                          if (res.success) {
                            setSelectedChat(prev => prev ? { ...prev, isMember: true, isInvite: false } : null);
                            fetchDialogs();
                            setConfirmModal({
                              title: 'Sucesso',
                              body: res.message || 'Você entrou no grupo com sucesso!',
                              onConfirm: () => setConfirmModal(null)
                            });
                          } else {
                            setConfirmModal({
                              title: 'Erro',
                              body: `Erro ao entrar: ${res.error}`,
                              onConfirm: () => setConfirmModal(null)
                            });
                          }
                        }}
                      >
                        ENTRAR NO {selectedChat.isChannel ? 'CANAL' : 'GRUPO'}
                      </button>
                    </div>
                  )
                )
              )}
            </>
          ) : (
            <div className="convo-empty fade-in">
              <div className="convo-empty-icon">✈</div>
              <h3>Nenhum chat selecionado</h3>
              <p>Escolha uma conversa na lista para ver o histórico e baixar mídias.</p>
            </div>
          )}
        </div>

        {/* ── Info panel ───────────────────────────────────────────── */}
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
                    <span className="info-stat-value">{messages.length}</span>
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
                  <div style={{ padding: '20px 0', textAlign: 'center' }}><span className="spinner small-spinner" /></div>
                ) : sharedMedia.length > 0 ? (
                  <div className="info-media-grid">
                    {sharedMedia.map(m => (
                      <div key={m.id} className="info-media-item">
                        <MessageMedia
                          chatId={selectedChat.id}
                          messageId={m.id}
                          isVideo={m.isVideo}
                          palette={palette}
                          density={density}
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
      </div>

      {/* Context menu portal */}
      {chatContextMenu && (
        <ContextMenu
          x={chatContextMenu.x}
          y={chatContextMenu.y}
          palette={palette}
          density={density}
          items={[
            {
              label: 'Ver informações',
              icon: <IconPanel />,
              onClick: () => {
                setSelectedChat(chatContextMenu.chat);
                setInfoOpen(true);
                setChatContextMenu(null);
              },
            },
            {
              label: 'Marcar como lida',
              icon: <IconCheck />,
              onClick: async () => {
                const chatId = chatContextMenu.chat.id;
                await telegramService.readHistory(chatId);
                setChats(prev => prev.map(c => c.id === chatId ? { ...c, unreadCount: 0 } : c));
                setChatContextMenu(null);
              },
              disabled: (chatContextMenu.chat.unreadCount ?? 0) === 0
            },
            {
              label: 'Silenciar notificações',
              icon: <IconBell />,
              onClick: async () => {
                const res = await telegramService.muteChat({ chatId: chatContextMenu.chat.id });
                if (res.success) {
                  setConfirmModal({
                    title: 'Silenciado',
                    body: `As notificações de "${chatContextMenu.chat.title}" foram silenciadas permanentemente.`,
                    onConfirm: () => setConfirmModal(null)
                  });
                } else {
                  alert(`Erro ao silenciar: ${res.error}`);
                }
                setChatContextMenu(null);
              },
            },
            { separator: true },
            {
              label: 'Sair do grupo',
              icon: <IconLogOut />,
              onClick: () => {
                const chat = chatContextMenu.chat;
                setChatContextMenu(null);
                setConfirmModal({
                  title: 'Sair do grupo',
                  body: `Tem certeza que deseja sair de "${chat.title}"? Você não poderá mais receber mensagens deste grupo.`,
                  danger: true,
                  onConfirm: async () => {
                    const res = await telegramService.leaveChat(chat.id);
                    if (res.success) {
                      setChats(prev => prev.filter(c => c.id !== chat.id));
                      if (selectedChat?.id === chat.id) setSelectedChat(null);
                      setConfirmModal(null);
                    } else {
                      setConfirmModal(prev => prev ? { ...prev, body: `Erro: ${res.error || 'Não foi possível sair do grupo.'}` } : null);
                    }
                  }
                });
              },
              disabled: !chatContextMenu.chat.isGroup && !chatContextMenu.chat.isChannel
            },
            {
              label: 'Limpar histórico',
              icon: <IconTrash />,
              onClick: () => {
                setChatContextMenu(null);
              },
              disabled: true
            },
          ]}
          onClose={() => setChatContextMenu(null)}
        />
      )}
      {msgContextMenu && (
        <ContextMenu
          x={msgContextMenu.x}
          y={msgContextMenu.y}
          palette={palette}
          density={density}
          items={[
            {
              label: 'Responder',
              icon: <IcoReply />,
              onClick: () => { setReplyTo(msgContextMenu.message); setMsgContextMenu(null); },
            },
            ...(msgContextMenu.message.text ? [{
              label: 'Copiar texto',
              icon: <IcoCopy />,
              onClick: () => { navigator.clipboard.writeText(msgContextMenu.message.text); setMsgContextMenu(null); },
            }] : []),
            ...(msgContextMenu.message.is_edited ? [{ separator: true as const }] : []),
            ...(msgContextMenu.message.is_edited ? [{
              label: 'Ver mensagem original',
              icon: <span style={{ fontSize: 14 }}>🕐</span>,
              onClick: () => handleViewOriginalMessage(msgContextMenu.message),
            }] : []),
          ]}
          onClose={() => setMsgContextMenu(null)}
        />
      )}
      {originalMsgModal && (
        <div className="original-msg-modal-overlay" onClick={() => setOriginalMsgModal(null)}>
          <div className="original-msg-modal" onClick={e => e.stopPropagation()}>
            <div className="original-msg-modal-header">
              <h3>Mensagem original</h3>
              <button className="icon-btn" onClick={() => setOriginalMsgModal(null)}>✕</button>
            </div>
            <div className="original-msg-modal-body">
              <div className="original-msg-label">Versão original</div>
              <div className="original-msg-text">{originalMsgModal.original.text || '(sem texto)'}</div>
              <div className="original-msg-divider" />
              <div className="original-msg-label">Versão atual</div>
              <div className="original-msg-text">{originalMsgModal.msg.text || '(sem texto)'}</div>
            </div>
          </div>
        </div>
      )}
      {isSettingsOpen && <Settings onClose={() => setIsSettingsOpen(false)} />}
      {confirmModal && (
        <div className="confirm-modal-overlay" onClick={() => setConfirmModal(null)}>
          <div className="confirm-modal" onClick={e => e.stopPropagation()}>
            <div className="confirm-modal-header">
              {confirmModal.danger ? <IconLogOut /> : <IconPanel />}
              <h3>{confirmModal.title}</h3>
            </div>
            <div className="confirm-modal-body">
              <p>{confirmModal.body}</p>
            </div>
            <div className="confirm-modal-footer">
              {!confirmModal.hideCancel && (
                <button className="confirm-modal-cancel" onClick={() => setConfirmModal(null)}>
                  Cancelar
                </button>
              )}
              <button
                className={`confirm-modal-confirm ${confirmModal.danger ? 'danger' : ''}`}
                onClick={confirmModal.onConfirm}
              >
                {confirmModal.confirmText || (confirmModal.danger ? 'Sair' : 'Confirmar')}
              </button>
            </div>
          </div>
        </div>
      )}

      {emojiPickerMsgId !== null && (() => {
        const pickerMsg = messages.find(m => m.id === emojiPickerMsgId);
        if (!pickerMsg) return null;
        return (
          <div
            className="emoji-picker-popover"
            style={{ left: emojiPickerPos.x, top: emojiPickerPos.y }}
            onClick={e => e.stopPropagation()}
          >
            {QUICK_REACTIONS.map(emoji => (
              <button key={emoji} type="button" className="emoji-react-btn" onClick={() => handleReact(pickerMsg, emoji)}>
                {emoji}
              </button>
            ))}
          </div>
        );
      })()}
      {imgContextMenu && selectedChat && (
        <ContextMenu
          x={imgContextMenu.x}
          y={imgContextMenu.y}
          palette={palette}
          density={density}
          items={[
            {
              label: 'Salvar imagem',
              icon: <IconDownload />,
              onClick: () => telegramService.saveMessageMediaFile({ chatId: selectedChat.id, messageId: imgContextMenu.msg.id }),
            },
            { separator: true },
            {
              label: 'Responder',
              icon: <IcoReply />,
              onClick: () => setReplyTo(imgContextMenu.msg),
            },
            {
              label: 'Encaminhar',
              icon: <IcoForward />,
              onClick: () => { },
            },
          ]}
          onClose={() => setImgContextMenu(null)}
        />
      )}
      {userContextMenu && selectedChat && (
        <ContextMenu
          x={userContextMenu.x}
          y={userContextMenu.y}
          palette={palette}
          density={density}
          items={[
            {
              label: `Buscar mídias de ${userContextMenu.senderName}`,
              icon: <IconSearch />,
              onClick: () => {
                const { senderId, senderName } = userContextMenu;
                setUserContextMenu(null);
                setSearchMediaUser({ senderId, senderName });
                triggerUserMediaSearch(senderId);
              },
            },
          ]}
          onClose={() => setUserContextMenu(null)}
        />
      )}
      {searchMediaUser && selectedChat && (
        <div className="search-media-modal-overlay" onClick={() => setSearchMediaUser(null)}>
          <div className="search-media-modal" onClick={e => e.stopPropagation()}>
            <div className="search-media-modal-header">
              <div className="search-media-user-info">
                <h3>Mídias Enviadas</h3>
                <span className="search-media-subtitle">por {searchMediaUser.senderName}</span>
              </div>
              <div className="search-media-header-actions">
                {searchMediaResults.length > 0 && (
                  <button
                    className="btn-download-all"
                    onClick={() => handleBulkDownload(searchMediaResults.map(r => r.id))}
                    title="Baixar todas as mídias da lista"
                  >
                    <IconDownload />
                    <span>Baixar Todos ({searchMediaResults.length})</span>
                  </button>
                )}
                <button className="icon-btn close-btn" onClick={() => setSearchMediaUser(null)}>✕</button>
              </div>
            </div>
            <div className="search-media-modal-body">
              {searchMediaLoading ? (
                <div className="search-media-loading-state">
                  <div className="premium-spinner"></div>
                  <span>Procurando fotos e vídeos...</span>
                </div>
              ) : searchMediaResults.length === 0 ? (
                <div className="search-media-empty-state">
                  <div className="empty-icon">📷</div>
                  <span>Nenhuma foto ou vídeo encontrado para este usuário no grupo.</span>
                </div>
              ) : (
                <div className="search-media-grid">
                  {searchMediaResults.map(item => (
                    <div key={item.id} className="search-media-grid-item">
                      <MessageMedia
                        chatId={selectedChat.id}
                        messageId={item.id}
                        isVideo={item.isVideo}
                        mediaSize={item.mediaSize}
                        palette={palette}
                        density={density}
                      />
                    </div>
                  ))}
                </div>
              )}
            </div>
          </div>
        </div>
      )}
      {bulkDownloadActive && bulkProgress && (
        <div className="save-multiple-progress-overlay">
          <div className="save-multiple-progress-card">
            <div className="save-multiple-progress-header">
              <div className="premium-spinner-container" style={{ position: 'relative', width: 64, height: 64, display: 'flex', alignItems: 'center', justifyContent: 'center', margin: '0 auto 10px' }}>
                <img src={appIcon} width="36" height="36" alt="Telegram Icon" style={{ borderRadius: '8px', zIndex: 2 }} />
                <div className="premium-spinner" style={{ position: 'absolute', inset: 0, width: '100%', height: '100%', border: '3px solid var(--bg-3)', borderTopColor: 'var(--accent)', borderRadius: '50%', animation: 'spin 1.2s linear infinite', zIndex: 1, boxShadow: '0 0 16px var(--accent-glow)' }}></div>
              </div>
              <h3>Baixando Mídias</h3>
              <p className="subtitle">Salvando arquivos no seu dispositivo...</p>
            </div>
            <div className="save-multiple-progress-body">
              <div className="progress-details">
                <span className="current-file">{bulkProgress.currentFile}</span>
                <span className="progress-fraction">
                  {Math.floor(bulkProgress.downloaded)} / {bulkProgress.total}
                </span>
              </div>
              <div className="progress-bar-container">
                <div
                  className="progress-bar-fill animated-glow"
                  style={{ width: `${Math.min(100, Math.round((bulkProgress.downloaded / bulkProgress.total) * 100))}%` }}
                ></div>
              </div>
              <div className="progress-percentage">
                {Math.min(100, Math.round((bulkProgress.downloaded / bulkProgress.total) * 100))}%
              </div>
            </div>
            <div className="save-multiple-progress-footer">
              <button className="btn-stop-download danger" onClick={handleStopBulkDownload}>
                Cancelar Download
              </button>
            </div>
          </div>
        </div>
      )}
    </>
  );
};
