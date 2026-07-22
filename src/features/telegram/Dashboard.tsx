// @ts-nocheck
import React, { useEffect, useState, useRef, useCallback, useMemo } from 'react';
import { createPortal } from 'react-dom';
import { invokeCommand as invoke } from '../../shared/platform/tauri';
import '../../styles/Dashboard.css';
import { ChatAvatar } from '../../components/ChatAvatar';
import { MessageMedia } from '../../components/MessageMedia';
import { ContextMenu, IcoDownload, IcoCopy, IcoForward, IcoReply } from '../../components/ContextMenu';
import { telegramService } from './TelegramService';
import { Settings } from './Settings';
import { Virtuoso } from 'react-virtuoso';
import appIcon from '../../../build/icon.png';
import { useAppearance } from '../appearance/AppearanceStore';
import { updateTwitterProfileChat } from './TwitterFakeChatStore';
import { getStoredTwitterCookies } from '../twitter/TwitterSettingsStore';
import { appStorage } from '../../shared/storage/appStorage';
import { writeClipboardText } from '../../shared/platform/clipboard';
import { debugLog, debugWarn } from '../../shared/debug/logger';
import { QUICK_REACTIONS, TOPIC_ICON_COLORS, hashColor } from './TelegramDashboardConstants';
import type { Chat, ChatFullInfo, ForumTopic, Message } from './TelegramDashboardTypes';
import { getTimelineItems, getTopicColor, ListContainer } from './DashboardHelpers';
import { DashboardChatList } from './DashboardChatList';
import { DashboardInfoPanel } from './DashboardInfoPanel';
import { DashboardMassDownloadPanel } from './DashboardMassDownloadPanel';
import { JoinChannelBar, MessageComposer, SelectionActionBar } from './DashboardComposer';
import {
  IconBack,
  IconBell,
  IconCheck,
  IconDownload,
  IconLogOut,
  IconMagic,
  IconMore,
  IconPanel,
  IconPlus,
  IconSearch,
  IconTrash,
} from './DashboardIcons';

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

interface DashboardProps {
  skipLogin?: boolean;
  onTelegramLoginRequest?: () => void;
}

export const Dashboard: React.FC<DashboardProps> = ({ skipLogin = false, onTelegramLoginRequest }) => {
  const PAGE_SIZE = 50;
  const [chats, setChats] = useState<Chat[]>([]);
  const [loading, setLoading] = useState(true);
  const [selectedChat, setSelectedChat] = useState<Chat | null>(null);
  const [messages, setMessages] = useState<Message[]>([]);
  const [visibleMediaIds, setVisibleMediaIds] = useState<Set<number>>(new Set());
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
  const messagesLoadSeqRef = useRef(0);
  const sharedMediaLoadSeqRef = useRef(0);

  const [isDownloadModalOpen, setIsDownloadModalOpen] = useState(false);
  const [isMenuOpen, setIsMenuOpen] = useState(false);
  const [updatingTwitterMessages, setUpdatingTwitterMessages] = useState(false);
  const [forumTopics, setForumTopics] = useState<ForumTopic[]>([]);
  const [loadingTopics, setLoadingTopics] = useState(false);
  const [selectedTopicId, setSelectedTopicId] = useState<string>('all');
  const [viewingTopic, setViewingTopic] = useState<ForumTopic | null>(null);

  const [folderPath, setFolderPath] = useState<string>('');
  const [splitByUser, setSplitByUser] = useState<boolean>(false);
  const [splitByAlbum, setSplitByAlbum] = useState<boolean>(false);
  const [albumSplitMode, setAlbumSplitMode] = useState<'separator' | 'comment'>('separator');
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

  const { palette, density } = useAppearance();
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
  const timelineItems = useMemo(() => getTimelineItems(messages), [messages]);
  const timelineFirstItemIndex = useMemo(
    () => Math.max(0, 100000 - timelineItems.length),
    [timelineItems.length]
  );
  const updateVisibleMediaIds = useCallback((range: { startIndex: number; endIndex: number }) => {
    const rawStart = Number(range.startIndex || 0);
    const rawEnd = Number(range.endIndex || rawStart);
    const start = Math.max(0, (rawStart >= timelineFirstItemIndex ? rawStart - timelineFirstItemIndex : rawStart) - 3);
    const end = Math.min(timelineItems.length - 1, (rawEnd >= timelineFirstItemIndex ? rawEnd - timelineFirstItemIndex : rawEnd) + 3);
    const ids = new Set<number>();

    for (let index = start; index <= end; index++) {
      const item = timelineItems[index];
      if (!item) continue;
      if (item.type === 'album') {
        item.messages?.forEach(message => {
          if (message.hasMedia) ids.add(Number(message.id));
        });
      } else if (item.message.hasMedia) {
        ids.add(Number(item.message.id));
      }
    }

    telegramService.cancelQueuedThumbnails({
      activeChatId: selectedChat?.id,
      keepMessageIds: ids,
    });
    telegramService.cancelQueuedFullMediaExceptChat(selectedChat?.id ?? null, ids);
    setVisibleMediaIds(ids);
  }, [selectedChat?.id, timelineFirstItemIndex, timelineItems]);

  const getChatKind = (chat: Chat) => {
    if (chat.isFakeTwitter || (typeof chat.id === 'string' && chat.id.startsWith('twitter_profile_'))) return 'twitter';
    if (chat.isGroup) return 'grupo';
    if (chat.isChannel) return 'canal';
    return 'conversa';
  };

  const isTwitterChat = (chat: Chat | null) => Boolean(chat?.isFakeTwitter || (typeof chat?.id === 'string' && chat.id.startsWith('twitter_profile_')));

  const getDownloadMeta = (msg: Message, senderName?: string | null) => ({
    chatTitle: selectedChat?.title,
    chatKind: selectedChat ? getChatKind(selectedChat) : undefined,
    topicTitle: viewingTopic && viewingTopic.id !== 0 ? viewingTopic.title : undefined,
    senderName: senderName || (msg.out ? 'Você' : msg.senderName || undefined),
    senderId: msg.senderId,
  });

  const normalizePeerId = (value: unknown) => String(value ?? '').replace(/[^\d-]/g, '');

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
      debugWarn('Handle link error:', err);
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
    const unsubscribeProgress = telegramService.onDownloadProgress((data) => {
      setDownloading(true);
      setProgress({ total: data.total, downloaded: data.downloaded, currentFile: data.currentFile, topicTitle: data.topicTitle, isScanning: data.isScanning, items: data.items });
      const currentFile = String(data.currentFile || '');
      if (currentFile.startsWith('Concluído') || currentFile.startsWith('Concluido') || currentFile.startsWith('Parado') || currentFile === 'Concluído!') {
        setTimeout(() => {
          setDownloading(false);
          setStopping(false);
        }, 300);
      }
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
      unsubscribeProgress();
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
    telegramService.cancelQueuedFullMediaExceptChat(selectedChat?.id ?? null);
    telegramService.cancelQueuedThumbnails({ activeChatId: selectedChat?.id ?? null });

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
        if (!selectedChat.hasTopics) loadMessages(selectedChat.id, 0, undefined, { refresh: true, latestKnownMessageDate: selectedChat.lastMessageDate });
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
    const unsubscribe = telegramService.onNewMessage(({ chatId, topicId, message }: any) => {
      if (!selectedChat || String(chatId) !== String(selectedChat.id)) return;

      const activeTopicId = viewingTopic && viewingTopic.id !== 0 ? viewingTopic.id : undefined;
      if (activeTopicId && Number(topicId || message.topicId || message.replyToMsgId || 0) !== Number(activeTopicId)) {
        return;
      }

      setMessages(current => {
        const byId = new Map<number, Message>();
        current.forEach(item => byId.set(Number(item.id), item));
        byId.set(Number(message.id), message);
        return Array.from(byId.values()).sort((a, b) => Number(a.id) - Number(b.id));
      });

      setChats(current => current.map(chat => String(chat.id) === String(chatId)
        ? {
          ...chat,
          lastMessageText: message.text || (message.hasMedia ? '' : chat.lastMessageText),
          lastMessageDate: message.date,
          lastMessageHasMedia: message.hasMedia,
          lastMessageIsVideo: message.isVideo,
          lastMessageIsPhoto: message.isPhoto,
        }
        : chat
      ));

      shouldScrollToBottomRef.current = true;
    });

    return () => unsubscribe();
  }, [selectedChat?.id, viewingTopic?.id]);

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
    if (!infoOpen || !selectedChat || selectedChat.isInvite) return;
    if (sharedMedia.length > 0 || loadingSharedMedia) return;
    fetchSharedMedia(selectedChat.id);
  }, [infoOpen, selectedChat?.id, sharedMedia.length, loadingSharedMedia]);

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
        const pendingFakeChatId = appStorage.get('plasma_twitter_pending_fake_chat');
        if (pendingFakeChatId) {
          const pendingChat = res.dialogs.find((chat: Chat) => chat.id === pendingFakeChatId);
          if (pendingChat) {
            setSelectedChat(pendingChat);
            appStorage.remove('plasma_twitter_pending_fake_chat');
          }
        }
        // Preload only the first visible-ish window in the background.
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
    const BATCH_SIZE = 4;
    const MAX_PRELOAD = 24;
    const targets = dialogs.slice(0, MAX_PRELOAD).filter(d => d.id && typeof d.id === 'string' && !d.id.startsWith('invite_'));
    telegramService.cancelQueuedAvatarsExcept(targets.map(chat => chat.id));
    for (let i = 0; i < targets.length; i += BATCH_SIZE) {
      const batch = targets.slice(i, i + BATCH_SIZE);
      await Promise.all(
        batch.map(d => telegramService.getAvatar(d.id, { priority: 'background' }).catch(() => null))
      );
      if (i + BATCH_SIZE < targets.length) {
        await new Promise(r => setTimeout(r, 80));
      }
    }
  };

  const fetchSharedMedia = async (chatId: string) => {
    const loadSeq = ++sharedMediaLoadSeqRef.current;
    let hasCachedMedia = false;
    try {
      const cached = await telegramService.getSharedMedia({ chatId, limit: 12, refresh: false });
      if (loadSeq !== sharedMediaLoadSeqRef.current) return;
      if (cached.success && cached.media?.length) {
        hasCachedMedia = true;
        setSharedMedia(cached.media);
      }

      if (!hasCachedMedia) setLoadingSharedMedia(true);

      const res = await telegramService.refreshSharedMedia({ chatId, limit: 12 });
      if (loadSeq !== sharedMediaLoadSeqRef.current) return;
      if (res.success) {
        const nextMedia = Array.isArray(res.media) ? res.media : [];
        setSharedMedia(nextMedia.length > 0 ? nextMedia : cached.media || []);
      }
    } catch (e) { debugWarn(e); }
    finally {
      if (loadSeq === sharedMediaLoadSeqRef.current) setLoadingSharedMedia(false);
    }
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
    } catch (e) { debugWarn(e); }
    finally { setLoadingFullInfo(false); }
  };

  const handleSelectTopic = (topic: ForumTopic) => {
    if (topicListRef.current) topicListScrollRef.current = topicListRef.current.scrollTop;
    setViewingTopic(topic);
    setSelectedTopicId(String(topic.id));
    loadMessages(selectedChat!.id, 0, topic.id, { refresh: true });

    if (topic.unreadCount > 0) {
      telegramService.readHistory(selectedChat!.id).catch(debugWarn);
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
    loadMessages(selectedChat!.id, 0, undefined, { refresh: true, latestKnownMessageDate: selectedChat?.lastMessageDate });
  };

  const loadMessages = async (chatId: string, offsetId = 0, topicId?: number, options: { silent?: boolean; refresh?: boolean; forceRefresh?: boolean; latestKnownMessageDate?: number | null } = {}) => {
    const loadSeq = ++messagesLoadSeqRef.current;
    if (!options.silent) setLoadingMessages(true);
    try {
      if (options.refresh && !offsetId) {
        const cached = await telegramService.getCachedMessages({ chatId, limit: PAGE_SIZE, topicId });
        if (loadSeq !== messagesLoadSeqRef.current) return;
        if (cached.success && cached.messages?.length) {
          setMessages(cached.messages);
          setHasMoreMessages(Boolean(cached.hasMore));
          setOldestMessageId(cached.oldestMessageId ?? null);
          setLoadingMessages(false);
          const latestKnownMessageDate = Number(options.latestKnownMessageDate || 0);
          const cacheHasKnownLatest = !latestKnownMessageDate || Number(cached.newestMessageDate || 0) >= latestKnownMessageDate;
          if (cached.isFresh && cacheHasKnownLatest && !options.forceRefresh) return;
        }
      }

      const res = await telegramService.getMessages({ chatId, limit: PAGE_SIZE, offsetId, topicId, refresh: options.refresh });
      if (loadSeq !== messagesLoadSeqRef.current) return;
      if (res.success && res.messages) {
        setMessages(res.messages);
        setHasMoreMessages(Boolean(res.hasMore));
        setOldestMessageId(res.oldestMessageId ?? null);
      }
    } catch (e) { debugWarn(e); }
    finally {
      if (loadSeq === messagesLoadSeqRef.current) setLoadingMessages(false);
    }
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
    } catch (e) { debugWarn(e); }
    finally { setLoadingMoreMessages(false); }
  };

  const handleSelectFolder = async () => {
    const res = await telegramService.selectFolder();
    if (res.success && res.folderPath) setFolderPath(res.folderPath);
  };

  const handleStartDownload = async () => {
    if (!selectedChat || !folderPath) return;
    const selectedTopic = forumTopics.find(topic => String(topic.id) === selectedTopicId) || null;
    setDownloading(true); setStopping(false); setProgress({
      total: 0,
      downloaded: 0,
      currentFile: 'Iniciando download...',
      topicTitle: selectedTopic?.title || null,
      isScanning: true,
      items: []
    });
    try {
      const res = await telegramService.startDownload({
        chatId: selectedChat.id, folderPath,
        topic: selectedTopic ? { id: selectedTopic.id, title: selectedTopic.title, topMessageId: selectedTopic.topMessageId } : null,
        splitByUser: !selectedChat.hasTopics ? splitByUser : false,
        splitByAlbum,
        albumSplitMode,
        chatMeta: {
          title: selectedChat.title,
          kind: getChatKind(selectedChat),
        }
      });
      if (!res.success) setError(res.error || 'Failed to start download');
    } catch (e: any) { setError(e.message || 'Unknown error'); }
    finally { setDownloading(false); setStopping(false); }
  };

  const handleStopDownload = async () => { setStopping(true); await telegramService.stopDownload(); };

  const triggerUserMediaSearch = async (userId: string) => {
    if (!selectedChat) return;
    setSearchMediaLoading(true);
    const localMediaResults = messages
      .filter(msg => msg.hasMedia && normalizePeerId(msg.senderId) === normalizePeerId(userId))
      .map(msg => ({
        id: msg.id,
        isVideo: msg.isVideo,
        mediaSize: msg.mediaSize ?? null,
      }));
    setSearchMediaResults(localMediaResults);
    try {
      const res = await telegramService.searchUserMedia({
        chatId: selectedChat.id,
        userId,
        limit: 100
      });
      if (res.success && res.media) {
        const merged = [...res.media, ...localMediaResults];
        const seen = new Set<number>();
        setSearchMediaResults(merged.filter(item => {
          if (seen.has(item.id)) return false;
          seen.add(item.id);
          return true;
        }));
      } else {
        debugWarn('Failed to search user media:', res.error);
      }
    } catch (err) {
      debugWarn('Error during media search:', err);
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
      debugWarn('Error during bulk download:', err);
      setBulkDownloadActive(false);
      setBulkProgress(null);
    }
  };

  const handleUpdateTwitterMessages = async () => {
    if (!selectedChat || updatingTwitterMessages) return;

    setUpdatingTwitterMessages(true);
    setIsMenuOpen(false);
    setError('');

    try {
      const username = selectedChat.id.replace(/^twitter_profile_/, '');
      const profile = await invoke('analyze_twitter_profile_native', {
        url: `https://x.com/${username}`,
        cookies: getStoredTwitterCookies().trim() || null,
      });
      const res = updateTwitterProfileChat(profile);
      if (!res.success) {
        setError(res.error || 'Não foi possível atualizar mensagens do Twitter/X.');
        return;
      }

      await fetchDialogs();
      await loadMessages(selectedChat.id, 0, undefined, { silent: true });
      telegramService.invalidateSharedMedia(selectedChat.id);
      if (infoOpen) fetchSharedMedia(selectedChat.id);
      setConfirmModal({
        title: res.addedCount > 0 ? 'Mensagens atualizadas' : 'Nada novo por aqui',
        body: res.addedCount > 0
          ? `${res.addedCount} ${res.addedCount === 1 ? 'nova mídia foi adicionada' : 'novas mídias foram adicionadas'} ao chat.`
          : 'Nenhuma mídia nova foi encontrada nesse perfil.',
        onConfirm: () => setConfirmModal(null),
      });
    } catch (err: any) {
      setError(err?.message || 'Falha ao atualizar mensagens do Twitter/X.');
    } finally {
      setUpdatingTwitterMessages(false);
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
        loadMessages(selectedChat.id, 0, topicId, { silent: true, refresh: true, forceRefresh: true });
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

  const handleForwardMessage = async (msg: Message) => {
    if (!selectedChat || isSending) return;
    const topicId = viewingTopic && viewingTopic.id !== 0 ? viewingTopic.id : undefined;

    setIsSending(true);
    try {
      const res = await telegramService.forwardMessage({
        chatId: selectedChat.id,
        messageId: msg.id,
        topicId,
      });

      if (res.success) {
        shouldScrollToBottomRef.current = true;
        loadMessages(selectedChat.id, 0, topicId, { silent: true, refresh: true, forceRefresh: true });
      } else {
        setError(res.error || 'Falha ao encaminhar mensagem.');
      }
    } catch (e: any) {
      setError(e.message || 'Erro ao encaminhar mensagem.');
    } finally {
      setIsSending(false);
      setMsgContextMenu(null);
      setImgContextMenu(null);
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
      debugWarn('Failed to send reaction:', err);
      // Revert or fetch messages again if needed
    }
  }, [selectedChat]);

  useEffect(() => {
    if (emojiPickerMsgId === null) return;
    const close = () => setEmojiPickerMsgId(null);
    document.addEventListener('click', close);
    return () => document.removeEventListener('click', close);
  }, [emojiPickerMsgId]);

  const handleJoinSelectedChat = async () => {
    if (!selectedChat) return;

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
  };

  const triggerJumpScroll = (targetMsgId: number, virtuosoIdx: number) => {
    debugLog('[TelegramEnchanted] Triggering jump scroll to index:', virtuosoIdx, 'for msg ID:', targetMsgId);
    if (virtuosoRef.current) {
      virtuosoRef.current.scrollToIndex({ index: virtuosoIdx, align: 'center' });
    }

    const targetId = `msg-${targetMsgId}`;
    let attemptsCount = 0;
    const pollAndAlign = () => {
      const domEl = document.getElementById(targetId);
      if (domEl) {
        debugLog('[TelegramEnchanted] Found target in DOM. Scrolling into center view.');
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
            debugWarn('[TelegramEnchanted] Polling failed to find target in DOM after 3 seconds.');
          }
        }
      }, 50);
    }
  };

  const handleJumpToMessage = async (replyToMsgId: number) => {
    let currentMessages = [...messagesRef.current];
    debugLog('[TelegramEnchanted] Jump to Message triggered. Target replyToMsgId:', replyToMsgId);
    debugLog('[TelegramEnchanted] Total loaded messages:', currentMessages.length);

    let originalMsgIdx = currentMessages.findIndex(m => Number(m.id) === Number(replyToMsgId));
    debugLog('[TelegramEnchanted] Message index in array:', originalMsgIdx);

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
            debugLog('[TelegramEnchanted] Target message not found in local feed. Loading older chunk... Attempt:', attempts + 1);
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
          debugWarn('Error loading older messages for jump:', e);
          pendingJumpToMsgIdRef.current = null;
        } finally {
          setLoadingMessages(false);
        }
      } else {
        debugWarn('[TelegramEnchanted] Target message not found in local feed and cannot load older.');
        setError('A mensagem original está muito antiga.');
      }
    }
  };

  const showComposer = !selectedChat?.hasTopics || (viewingTopic && viewingTopic.id !== 0);

  if (loading) return (
    <div className="full-screen-loader fade-in">
      <div className="loader-content">
        <div className="modern-loader" role="status" aria-label="Carregando conversas" />
      </div>
    </div>
  );

  return (
    <>
      <div className={`app ${selectedChat ? 'has-selected-chat' : ''}`} data-palette={palette} data-density={density}>

        <DashboardChatList
          activeFolder={activeFolder}
          chatSearch={chatSearch}
          chats={chats}
          error={error}
          filteredChats={filteredChats}
          isSearchOpen={isSearchOpen}
          isSettingsMenuOpen={isSettingsMenuOpen}
          loading={loading}
          selectedChat={selectedChat}
          skipLogin={skipLogin}
          formatMessageTime={formatMessageTime}
          getChatKind={getChatKind}
          onTelegramLoginRequest={onTelegramLoginRequest}
          readChatHistory={telegramService.readHistory.bind(telegramService)}
          setActiveFolder={setActiveFolder}
          setChatContextMenu={setChatContextMenu}
          setChatSearch={setChatSearch}
          setChats={setChats}
          setError={setError}
          setIsSearchOpen={setIsSearchOpen}
          setIsSettingsMenuOpen={setIsSettingsMenuOpen}
          setIsSettingsOpen={setIsSettingsOpen}
          setSelectedChat={setSelectedChat}
        />

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
                  <button
                    className="icon-btn mobile-chat-back"
                    onClick={() => {
                      setSelectedChat(null);
                      setViewingTopic(null);
                      setInfoOpen(false);
                      setIsSelectionMode(false);
                      setSelectedMessageIds([]);
                    }}
                    title="Voltar para chats"
                  >
                    <IconBack />
                  </button>
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
                        {isTwitterChat(selectedChat) && (
                          <div
                            className="dropdown-item"
                            onClick={handleUpdateTwitterMessages}
                            style={updatingTwitterMessages ? { opacity: 0.6, pointerEvents: 'none' } : undefined}
                          >
                            <IconMagic />
                            <span>{updatingTwitterMessages ? 'Atualizando...' : 'Atualizar mensagens'}</span>
                          </div>
                        )}
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
                  <DashboardMassDownloadPanel
                    albumSplitMode={albumSplitMode}
                    downloading={downloading}
                    filteredTopics={filteredTopics}
                    folderPath={folderPath}
                    forumTopics={forumTopics}
                    hasTopics={Boolean(selectedChat.hasTopics)}
                    isTopicDropdownOpen={isTopicDropdownOpen}
                    loadingTopics={loadingTopics}
                    progress={progress}
                    progressDetailsListRef={progressDetailsListRef}
                    selectedTopicId={selectedTopicId}
                    showDetailedProgress={showDetailedProgress}
                    splitByAlbum={splitByAlbum}
                    splitByUser={splitByUser}
                    stopping={stopping}
                    topicSearch={topicSearch}
                    handleSelectFolder={handleSelectFolder}
                    handleStartDownload={handleStartDownload}
                    handleStopDownload={handleStopDownload}
                    setAlbumSplitMode={setAlbumSplitMode}
                    setIsDownloadModalOpen={setIsDownloadModalOpen}
                    setIsTopicDropdownOpen={setIsTopicDropdownOpen}
                    setSelectedTopicId={setSelectedTopicId}
                    setShowDetailedProgress={setShowDetailedProgress}
                    setSplitByAlbum={setSplitByAlbum}
                    setSplitByUser={setSplitByUser}
                    setTopicSearch={setTopicSearch}
                  />
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
                    <div className="loader-surface" role="status" aria-label="Carregando tópicos">
                      <span className="modern-loader small" />
                    </div>
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
                    <div className="loader-surface" style={{ zIndex: 1 }} role="status" aria-label="Carregando mensagens">
                      <span className="modern-loader small" />
                    </div>
                  ) : messages.length === 0 ? (
                    <div className="messages-empty" style={{ zIndex: 1 }}>Nenhuma mensagem encontrada.</div>
                  ) : (
                    <Virtuoso
                      ref={virtuosoRef}
                      style={{ height: '100%', width: '100%', outline: 'none', zIndex: 1 }}
                      data={timelineItems}
                      firstItemIndex={timelineFirstItemIndex}
                      rangeChanged={updateVisibleMediaIds}
                      startReached={loadOlderMessages}
                      components={{
                        List: ListContainer,
                        Header: () => {
                          if (!hasMoreMessages) return null;
                          return (
                            <div className="messages-load-more" style={{ display: 'flex', justifyContent: 'center', padding: '16px 0' }}>
                              {loadingMoreMessages ? (
                                <div className="loader-surface compact" role="status" aria-label="Carregando mensagens anteriores">
                                  <span className="modern-loader small" />
                                </div>
                              ) : (
                                <div style={{ height: '24px' }} />
                              )}
                            </div>
                          );
                        }
                      }}
                      itemContent={(index, item) => {
                        const dataIndex = index - timelineFirstItemIndex;
                        const prevItem = timelineItems[dataIndex - 1];
                        const prev = prevItem?.message;
                        const msg = item.message;
                        const continued = isContinued(msg, prev);
                        const dayBreak = isNewMessageDay(msg, prev);
                        const color = msg.out ? 'cyan' : hashColor(msg.senderId || msg.id.toString());
                        const displayName = msg.out ? 'Você' : (msg.senderName || (msg.senderId ? `ID ${msg.senderId.slice(-6)}` : 'Desconhecido'));

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
                                            thumbnailUrl={albumMsg.thumbnailUrl}
                                            mediaPriority={visibleMediaIds.has(albumMsg.id) ? 'visible' : 'background'}
                                            palette={palette}
                                            density={density}
                                            downloadMeta={getDownloadMeta(albumMsg, albumMsg.out ? 'Você' : albumMsg.senderName || displayName)}
                                            selectionMode={isSelectionMode}
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
                                          thumbnailUrl={msg.thumbnailUrl}
                                          mediaPriority={visibleMediaIds.has(msg.id) ? 'visible' : 'background'}
                                          palette={palette}
                                          density={density}
                                          downloadMeta={getDownloadMeta(msg, displayName)}
                                          selectionMode={isSelectionMode}
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
                                  onClick={(e) => { e.stopPropagation(); handleForwardMessage(msg); }}
                                >→</button>
                                {item.type === 'album' ? (
                                  <button
                                    type="button" className="icon-btn" title="Salvar todas as mídias"
                                    onClick={async (e) => {
                                      e.stopPropagation();
                                      if (item.messages) {
                                        for (const albumMsg of item.messages) {
                                          try {
                                            await telegramService.saveMessageMediaFile({
                                              chatId: selectedChat.id,
                                              messageId: albumMsg.id,
                                              downloadMeta: getDownloadMeta(albumMsg, albumMsg.out ? 'Você' : albumMsg.senderName || displayName),
                                            });
                                          } catch (err) {
                                            debugWarn(err);
                                          }
                                        }
                                      }
                                    }}
                                  >⤓</button>
                                ) : msg.hasMedia ? (
                                  <button
                                    type="button" className="icon-btn" title="Salvar"
                                    onClick={(e) => {
                                      e.stopPropagation();
                                      telegramService.saveMessageMediaFile({
                                        chatId: selectedChat.id,
                                        messageId: msg.id,
                                        downloadMeta: getDownloadMeta(msg, displayName),
                                      });
                                    }}
                                  >⤓</button>
                                ) : msg.text ? (
                                  <button
                                    type="button" className="icon-btn" title="Copiar texto"
                                    onClick={(e) => { e.stopPropagation(); writeClipboardText(msg.text); }}
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
                <SelectionActionBar
                  selectedMessageIds={selectedMessageIds}
                  handleBulkDownload={handleBulkDownload}
                  setIsSelectionMode={setIsSelectionMode}
                  setSelectedMessageIds={setSelectedMessageIds}
                />
              ) : (
                showComposer && (
                  selectedChat.isMember !== false ? (
                    <MessageComposer
                      inputText={inputText}
                      isSending={isSending}
                      replyTo={replyTo}
                      selectedFile={selectedFile}
                      sendProgress={sendProgress}
                      handleSelectFile={handleSelectFile}
                      handleSend={handleSend}
                      setInputText={setInputText}
                      setReplyTo={setReplyTo}
                      setSelectedFile={setSelectedFile}
                    />
                  ) : (
                    <JoinChannelBar
                      fullChatInfo={fullChatInfo}
                      selectedChat={selectedChat}
                      onJoin={handleJoinSelectedChat}
                    />
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

        <DashboardInfoPanel
          density={density}
          forumTopics={forumTopics}
          fullChatInfo={fullChatInfo}
          infoOpen={infoOpen}
          loadingFullInfo={loadingFullInfo}
          loadingSharedMedia={loadingSharedMedia}
          messagesCount={messages.length}
          palette={palette}
          selectedChat={selectedChat}
          sharedMedia={sharedMedia}
          getChatKind={getChatKind}
          getDownloadMeta={getDownloadMeta}
          setInfoOpen={setInfoOpen}
        />
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
              onClick: () => { writeClipboardText(msgContextMenu.message.text); setMsgContextMenu(null); },
            }] : []),
            {
              label: 'Encaminhar',
              icon: <IcoForward />,
              onClick: () => handleForwardMessage(msgContextMenu.message),
            },
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
      {isSettingsOpen && createPortal(
        <Settings onClose={() => setIsSettingsOpen(false)} />,
        document.body
      )}
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
              label: 'Salvar como...',
              icon: <IconDownload />,
              onClick: () => telegramService.saveMessageMediaFile({
                chatId: selectedChat.id,
                messageId: imgContextMenu.msg.id,
                saveAs: true,
                downloadMeta: getDownloadMeta(
                  imgContextMenu.msg,
                  imgContextMenu.msg.out ? 'Você' : imgContextMenu.msg.senderName || undefined
                ),
              }),
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
              onClick: () => handleForwardMessage(imgContextMenu.msg),
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
                null
              ) : (
                <div className="search-media-grid">
                  {searchMediaResults.map(item => (
                    <div key={item.id} className="search-media-grid-item">
                      <MessageMedia
                        chatId={selectedChat.id}
                        messageId={item.id}
                        isVideo={item.isVideo}
                        mediaSize={item.mediaSize}
                        thumbnailUrl={item.thumbnailUrl}
                        palette={palette}
                        density={density}
                        downloadMeta={getDownloadMeta(item as Message)}
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
