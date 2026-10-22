import { platformFetch as tauriFetch } from '../../shared/platform/http';
import { debugLog, debugWarn } from '../../shared/debug/logger';
import { mediaCache } from './MediaCacheService';
import {
  findTwitterFakeMessage,
  getTwitterFakeChat,
  isTwitterFakeChatId,
  type TwitterFakeChat,
  type TwitterFakeMessage,
} from './TwitterFakeChatStore';

interface TwitterFakeMediaProgress {
  chatId: unknown;
  messageId: unknown;
  progress: number;
  stage: 'downloading' | 'ready' | 'failed';
}

interface TwitterFakeMediaResult {
  success: boolean;
  filePath?: string;
  streamUrl?: string;
  error?: string;
}

export class TelegramTwitterFakeBridge {
  constructor(
    private readonly emitMediaProgress: (data: TwitterFakeMediaProgress) => void,
  ) {}

  isFakeChat(chatId: unknown): boolean {
    return isTwitterFakeChatId(chatId);
  }

  getFakeChat(chatId: unknown): TwitterFakeChat | null {
    return getTwitterFakeChat(chatId);
  }

  findFakeMessage(chatId: unknown, messageId: unknown): TwitterFakeMessage | null {
    return findTwitterFakeMessage(chatId, messageId);
  }

  async downloadFakeMedia(chatId: unknown, messageId: unknown, message: TwitterFakeMessage): Promise<TwitterFakeMediaResult> {
    if (!message.url) return { success: false, error: 'Mídia sem URL.' };

    try {
      const cacheKey = `twitter_fake_${chatId}_${messageId}_full`;
      const cachedUrl = await mediaCache.getMedia(cacheKey, message.isVideo ? 'video/mp4' : undefined);
      if (cachedUrl) return { success: true, filePath: cachedUrl, streamUrl: cachedUrl };

      this.emitMediaProgress({ chatId, messageId, progress: 1, stage: 'downloading' });

      debugLog(`[TelegramService] Downloading Twitter/X media from: ${message.url}`);
      const response = await tauriFetch(message.url, {
        method: 'GET',
        headers: {
          'Referer': 'https://x.com/',
          'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36',
        },
      });

      if (!response.ok) {
        throw new Error(`Twitter/X media HTTP ${response.status}`);
      }

      const buffer = await response.arrayBuffer();
      const contentType = message.isVideo ? 'video/mp4' : (response.headers.get('content-type') || 'image/jpeg');

      const filePath = await mediaCache.saveMedia(cacheKey, buffer, contentType);
      this.emitMediaProgress({ chatId, messageId, progress: 100, stage: 'ready' });
      return { success: true, filePath, streamUrl: filePath };
    } catch (error) {
      debugWarn("[TelegramService] Failed to download Twitter/X media:", error);
      this.emitMediaProgress({ chatId, messageId, progress: 0, stage: 'failed' });
      return { success: false, error: error instanceof Error ? error.message : String(error) };
    }
  }
}
