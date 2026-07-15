// @ts-nocheck
import { platformFetch as tauriFetch } from '../../shared/platform/http';
import { debugLog } from '../../shared/debug/logger';
import { mediaCache } from './MediaCacheService';
import {
  findTwitterFakeMessage,
  getTwitterFakeChat,
  isTwitterFakeChatId,
  type TwitterFakeMessage,
} from './TwitterFakeChatStore';

export class TelegramTwitterFakeBridge {
  constructor(
    private readonly emitMediaProgress: (data: any) => void,
  ) {}

  isFakeChat(chatId: any) {
    return isTwitterFakeChatId(chatId);
  }

  getFakeChat(chatId: any) {
    return getTwitterFakeChat(chatId);
  }

  findFakeMessage(chatId: any, messageId: any) {
    return findTwitterFakeMessage(chatId, messageId);
  }

  async downloadFakeMedia(chatId: any, messageId: any, message: TwitterFakeMessage) {
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
    } catch (error: any) {
      console.error("[TelegramService] Failed to download Twitter/X media:", error);
      this.emitMediaProgress({ chatId, messageId, progress: 0, stage: 'failed' });
      return { success: false, error: error.message || String(error) };
    }
  }
}
