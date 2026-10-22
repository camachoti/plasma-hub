import { appStorage } from '../../shared/storage/appStorage';

export const TWITTER_FAKE_CHATS_KEY = 'plasma_twitter_fake_chats';
export const TWITTER_FAKE_PENDING_CHAT_KEY = 'plasma_twitter_pending_fake_chat';
export const TWITTER_FAKE_CHAT_EVENT = 'plasma-twitter-fake-chat-created';

export interface TwitterFakeMessage {
  id: number;
  url?: string;
  thumbnailUrl?: string | null;
  isVideo: boolean;
  date: number;
  text: string;
  mediaSize?: number | null;
}

export interface TwitterFakeChat {
  id: string;
  username: string;
  title: string;
  avatarUrl?: string | null;
  thumbnailUrl?: string | null;
  originalUrl: string;
  createdAt: number;
  messages: TwitterFakeMessage[];
}

interface TwitterProfileMediaItemInput {
  url: string;
  thumbnailUrl?: string | null;
  isVideo?: boolean;
}

export function isTwitterFakeChatId(chatId: unknown) {
  return typeof chatId === 'string' && chatId.startsWith('twitter_profile_');
}

export function readTwitterFakeChats(): TwitterFakeChat[] {
  try {
    const raw = appStorage.get(TWITTER_FAKE_CHATS_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    return Array.isArray(parsed) ? parsed : [];
  } catch {
    return [];
  }
}

export function writeTwitterFakeChats(chats: TwitterFakeChat[]) {
  appStorage.set(TWITTER_FAKE_CHATS_KEY, JSON.stringify(chats));
}

export function getTwitterFakeChat(chatId: unknown): TwitterFakeChat | null {
  if (!isTwitterFakeChatId(chatId)) return null;
  return readTwitterFakeChats().find(chat => chat.id === chatId) ?? null;
}

export function findTwitterFakeMessage(chatId: unknown, messageId: unknown) {
  const chat = getTwitterFakeChat(chatId);
  if (!chat) return null;
  return chat.messages.find(message => Number(message.id) === Number(messageId)) ?? null;
}

export function twitterFakeDialogs() {
  return readTwitterFakeChats().map(chat => {
    const last = chat.messages[chat.messages.length - 1];
    return {
      id: chat.id,
      title: chat.title,
      date: last?.date ?? chat.createdAt,
      unreadCount: 0,
      isGroup: false,
      isChannel: false,
      hasTopics: false,
      lastMessageText: last?.text || `${chat.messages.length} mídias importadas do X`,
      lastMessageDate: last?.date ?? chat.createdAt,
      lastMessageHasMedia: Boolean(last?.url),
      lastMessageIsVideo: Boolean(last?.isVideo),
      lastMessageIsPhoto: Boolean(last?.url && !last.isVideo),
      isFakeTwitter: true,
    };
  });
}

export function createTwitterProfileChat(profile: any) {
  const username = String(profile?.username || '').replace(/^@/, '');
  if (!username) return { success: false, error: 'Perfil do Twitter/X inválido.' };

  const mediaItems: TwitterProfileMediaItemInput[] = Array.isArray(profile.mediaItems) && profile.mediaItems.length > 0
    ? profile.mediaItems.filter((item: TwitterProfileMediaItemInput) => item?.url)
    : (Array.isArray(profile.mediaUrls) ? profile.mediaUrls.filter(Boolean).map((url: string) => ({
        url,
        thumbnailUrl: url.includes('video.twimg.com/') ? profile.thumbnailUrl || null : url,
        isVideo: /\.mp4(?:[?#]|$)/i.test(url) || url.includes('video.twimg.com/'),
      })) : []);

  if (mediaItems.length === 0) {
    return { success: false, error: 'Nenhuma mídia encontrada para criar o chat.' };
  }

  const now = Math.floor(Date.now() / 1000);
  const chatId = `twitter_profile_${username.toLowerCase()}`;
  const introMessage: TwitterFakeMessage = {
    id: 1,
    isVideo: false,
    date: now - mediaItems.length * 60,
    text: [
      `Perfil importado do Twitter/X: @${username}`,
      profile.displayName ? `Nome: ${profile.displayName}` : null,
      `${mediaItems.length} mídias coletadas`,
      profile.originalUrl || `https://x.com/${username}`,
    ].filter(Boolean).join('\n'),
    mediaSize: null,
  };
  const mediaMessages = mediaItems.map((item, index: number) => {
    const isVideo = Boolean(item.isVideo) || /\.mp4(?:[?#]|$)/i.test(item.url) || item.url.includes('video.twimg.com/');
    return {
      id: index + 2,
      url: item.url,
      thumbnailUrl: item.thumbnailUrl || (!isVideo ? item.url : null),
      isVideo,
      date: now - Math.max(0, mediaItems.length - index - 1) * 60,
      text: `${isVideo ? 'Vídeo' : 'Imagem'} ${index + 1} de ${mediaItems.length} · https://x.com/${username}`,
      mediaSize: null,
    };
  });

  const chat: TwitterFakeChat = {
    id: chatId,
    username,
    title: profile.displayName || `@${username}`,
    avatarUrl: profile.avatarUrl || null,
    thumbnailUrl: profile.thumbnailUrl || null,
    originalUrl: profile.originalUrl || `https://x.com/${username}`,
    createdAt: now,
    messages: [introMessage, ...mediaMessages],
  };

  const chats = readTwitterFakeChats().filter(existing => existing.id !== chatId);
  chats.unshift(chat);
  writeTwitterFakeChats(chats);
  appStorage.set(TWITTER_FAKE_PENDING_CHAT_KEY, chatId);
  window.dispatchEvent(new CustomEvent(TWITTER_FAKE_CHAT_EVENT, { detail: { chatId } }));
  return { success: true, chatId, mediaCount: mediaItems.length };
}

export function updateTwitterProfileChat(profile: any) {
  const username = String(profile?.username || '').replace(/^@/, '');
  const chatId = `twitter_profile_${username.toLowerCase()}`;
  if (!username || !isTwitterFakeChatId(chatId)) {
    return { success: false, error: 'Perfil do Twitter/X inválido.' };
  }

  const chats = readTwitterFakeChats();
  const chatIndex = chats.findIndex(chat => chat.id === chatId);
  if (chatIndex < 0) {
    return { success: false, error: 'Chat importado do Twitter/X não encontrado.' };
  }

  const mediaItems: TwitterProfileMediaItemInput[] = Array.isArray(profile.mediaItems) && profile.mediaItems.length > 0
    ? profile.mediaItems.filter((item: TwitterProfileMediaItemInput) => item?.url)
    : (Array.isArray(profile.mediaUrls) ? profile.mediaUrls.filter(Boolean).map((url: string) => ({
        url,
        thumbnailUrl: url.includes('video.twimg.com/') ? profile.thumbnailUrl || null : url,
        isVideo: /\.mp4(?:[?#]|$)/i.test(url) || url.includes('video.twimg.com/'),
      })) : []);

  const chat = chats[chatIndex];
  const existingUrls = new Set(chat.messages.map(message => message.url).filter(Boolean));
  const newItems = mediaItems.filter(item => item.url && !existingUrls.has(item.url));

  if (newItems.length === 0) {
    chats[chatIndex] = {
      ...chat,
      title: profile.displayName || chat.title,
      avatarUrl: profile.avatarUrl || chat.avatarUrl || null,
      thumbnailUrl: profile.thumbnailUrl || chat.thumbnailUrl || null,
      originalUrl: profile.originalUrl || chat.originalUrl,
    };
    writeTwitterFakeChats(chats);
    return { success: true, chatId, addedCount: 0 };
  }

  const now = Math.floor(Date.now() / 1000);
  const nextId = Math.max(0, ...chat.messages.map(message => Number(message.id) || 0)) + 1;
  const newMessages = newItems.map((item, index) => {
    const isVideo = Boolean(item.isVideo) || /\.mp4(?:[?#]|$)/i.test(item.url) || item.url.includes('video.twimg.com/');
    return {
      id: nextId + index,
      url: item.url,
      thumbnailUrl: item.thumbnailUrl || (!isVideo ? item.url : null),
      isVideo,
      date: now + index,
      text: `${isVideo ? 'Vídeo' : 'Imagem'} nova · https://x.com/${username}`,
      mediaSize: null,
    };
  });

  chats[chatIndex] = {
    ...chat,
    title: profile.displayName || chat.title,
    avatarUrl: profile.avatarUrl || chat.avatarUrl || null,
    thumbnailUrl: profile.thumbnailUrl || chat.thumbnailUrl || null,
    originalUrl: profile.originalUrl || chat.originalUrl,
    messages: [...chat.messages, ...newMessages],
  };
  writeTwitterFakeChats(chats);
  window.dispatchEvent(new CustomEvent(TWITTER_FAKE_CHAT_EVENT, { detail: { chatId } }));
  return { success: true, chatId, addedCount: newItems.length };
}
