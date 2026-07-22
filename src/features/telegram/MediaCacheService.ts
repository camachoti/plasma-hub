import { get, set, del, clear } from 'idb-keyval';
import { debugLog, debugWarn } from '../../shared/debug/logger';
import { runtimeCapabilities } from '../../shared/platform/runtime';
import { invokeCommand } from '../../shared/platform/tauri';

export interface CacheItemInfo {
  key: string;
  size: number;
  mimeType: string;
  type: 'avatar' | 'media' | 'message';
  addedAt: number;
  lastAccessed: number;
  nativeFilePath?: string;
}

export interface CacheSettings {
  maxCacheSize: number;
  avatarRefreshHours: number;
  downloadWorkers: number;
}

export interface MessageCacheMeta {
  lastFetchedAt?: number;
}

export type MediaAssetState = 'empty' | 'partial' | 'complete' | 'native';

export interface MediaAssetSegment {
  offset: number;
  length: number;
}

export interface MediaAssetMeta {
  key: string;
  state: MediaAssetState;
  totalBytes?: number;
  downloadedBytes?: number;
  mimeType?: string;
  fileName?: string;
  nativeFilePath?: string;
  completedAt?: number;
  updatedAt?: number;
  segments?: MediaAssetSegment[];
}

export interface MediaDownloadProgress {
  percent: number;
  downloadedBytes?: number;
  totalBytes?: number;
  stage?: string;
}

export class MediaCacheService {
  private memoryCache = new Map<string, string>();
  private registry = new Map<string, CacheItemInfo>();
  private registryLoaded = false;

  private async saveRegistry(): Promise<void> {
    try {
      const obj = Object.fromEntries(this.registry.entries());
      await set('cache_registry', obj);
    } catch (e) {
      debugWarn("Failed to save cache registry", e);
    }
  }

  private async ensureRegistryLoaded(): Promise<void> {
    if (this.registryLoaded) return;
    
    try {
      const regObj = await get<Record<string, any>>('cache_registry');
      if (regObj) {
        this.registry = new Map(Object.entries(regObj));
      } else {
        this.registry = new Map();
        await this.saveRegistry();
      }
    } catch (e) {
      debugWarn("Failed to load cache registry, starting empty", e);
      this.registry = new Map();
    }
    this.registryLoaded = true;
  }

  /**
   * Retrieves a media file from memory cache or IndexedDB.
   * If found in IDB but not memory, it creates a new Blob URL.
   */
  async getMedia(key: string, mimeType?: string): Promise<string | null> {
    await this.ensureRegistryLoaded();

    // 1. Check Memory Cache (instant)
    if (this.memoryCache.has(key)) {
      const info = this.registry.get(key);
      if (mimeType && info?.mimeType && info.mimeType !== mimeType) {
        URL.revokeObjectURL(this.memoryCache.get(key)!);
        this.memoryCache.delete(key);
      } else {
        if (info) {
          info.lastAccessed = Date.now();
          this.saveRegistry().catch(() => {});
        }
        return this.memoryCache.get(key)!;
      }
    }

    // 2. Check IndexedDB (persistent)
    try {
      const buffer = await get<ArrayBuffer | Uint8Array | any>(key);
      if (buffer) {
        // Retrieve stored mimeType: try registry first (fast), fallback to IDB _mime (backwards compatibility)
        const info = this.registry.get(key);
        let resolvedMimeType = mimeType || info?.mimeType;
        
        if (!resolvedMimeType) {
          resolvedMimeType = await get<string>(`${key}_mime`) || 'application/octet-stream';
        }

        const normalizedBuffer = this.toArrayBuffer(buffer);
        const blob = new Blob([normalizedBuffer], { type: resolvedMimeType });
        const url = URL.createObjectURL(blob);
        
        // Save to memory cache for subsequent instant access
        this.memoryCache.set(key, url);

        // Update registry metadata
        if (info) {
          info.lastAccessed = Date.now();
          if (mimeType && info.mimeType !== resolvedMimeType) {
            info.mimeType = resolvedMimeType;
            set(`${key}_mime`, resolvedMimeType).catch(() => {});
          }
        } else {
          // If missing in registry (e.g. legacy cached item), add it
          const size = normalizedBuffer.byteLength;
          this.registry.set(key, {
            key,
            size,
            mimeType: resolvedMimeType,
            type: key.startsWith('avatar_') ? 'avatar' : 'media',
            addedAt: Date.now(),
            lastAccessed: Date.now()
          });
        }
        this.saveRegistry().catch(() => {});

        return url;
      }
    } catch (e) {
      debugWarn("Failed to get media from IndexedDB cache", e);
    }

    return null;
  }

  async getCacheItemInfo(key: string): Promise<CacheItemInfo | null> {
    await this.ensureRegistryLoaded();
    return this.registry.get(key) || null;
  }

  /**
   * Saves a media file buffer to IndexedDB and Memory Cache.
   * Returns the generated Blob URL.
   */
  async saveMedia(key: string, buffer: ArrayBuffer | Uint8Array | any, mimeType?: string): Promise<string> {
    await this.ensureRegistryLoaded();
    const normalizedBuffer = this.toArrayBuffer(buffer);

    try {
      // Save to IndexedDB
      await set(key, normalizedBuffer);
      if (mimeType) {
        await set(`${key}_mime`, mimeType);
      }
    } catch (e) {
      debugWarn("Error initiating IDB save", e);
    }

    // Create Blob and save to Memory Cache
    const resolvedMime = mimeType || 'application/octet-stream';
    const blob = new Blob([normalizedBuffer], { type: resolvedMime });
    const url = URL.createObjectURL(blob);
    this.memoryCache.set(key, url);

    // Update registry
    const size = normalizedBuffer.byteLength;
    this.registry.set(key, {
      key,
      size,
      mimeType: resolvedMime,
      type: key.startsWith('avatar_') ? 'avatar' : 'media',
      addedAt: Date.now(),
      lastAccessed: Date.now()
    });
    await this.saveRegistry();
    await this.saveMediaAssetMeta(key, {
      state: 'complete',
      totalBytes: size,
      downloadedBytes: size,
      mimeType: resolvedMime,
      completedAt: Date.now(),
      segments: [],
    });

    // Evict items if registry size exceeds the maximum limit
    this.evictIfNeeded().catch(e => debugWarn("Failed to evict cache", e));

    return url;
  }

  async saveNativeFileReference(key: string, nativeFilePath: string, mimeType?: string): Promise<void> {
    await this.ensureRegistryLoaded();

    const existing = this.registry.get(key);
    this.registry.set(key, {
      key,
      size: existing?.size ?? 0,
      mimeType: mimeType || existing?.mimeType || 'application/octet-stream',
      type: key.startsWith('avatar_') ? 'avatar' : 'media',
      addedAt: existing?.addedAt ?? Date.now(),
      lastAccessed: Date.now(),
      nativeFilePath,
    });
    await this.saveMediaAssetMeta(key, {
      state: 'native',
      mimeType: mimeType || existing?.mimeType || 'application/octet-stream',
      nativeFilePath,
      completedAt: Date.now(),
    });
    await this.saveRegistry();
  }

  /**
   * Retrieves the raw ArrayBuffer for a media key directly from IndexedDB.
   */
  async getMediaBuffer(key: string): Promise<ArrayBuffer | null> {
    try {
      const buffer = await get<ArrayBuffer | Uint8Array | any>(key);
      if (buffer) {
        return this.toArrayBuffer(buffer);
      }
    } catch (e) {
      debugWarn("Failed to get media buffer from IndexedDB", e);
    }
    return null;
  }

  private toArrayBuffer(value: ArrayBuffer | Uint8Array | any): ArrayBuffer {
    if (value instanceof ArrayBuffer) return value;
    if (ArrayBuffer.isView(value)) {
      return value.buffer.slice(value.byteOffset, value.byteOffset + value.byteLength);
    }
    if (value?.buffer instanceof ArrayBuffer && typeof value.byteLength === 'number') {
      return value.buffer.slice(value.byteOffset || 0, (value.byteOffset || 0) + value.byteLength);
    }
    if (Array.isArray(value)) {
      return new Uint8Array(value).buffer;
    }
    return new Uint8Array(value || []).buffer;
  }

  // --- Media Asset Cache ---
  private mediaAssetMetaKey(key: string) {
    return `${key}_asset_meta`;
  }

  private mediaSegmentKey(key: string, offset: number, length: number) {
    return `${key}_segment_${offset}_${length}`;
  }

  private canUseNativeMessageCache() {
    return runtimeCapabilities.isTauri;
  }

  private async getNativeMessages(cacheKey: string): Promise<{ messages: any[]; meta: MessageCacheMeta } | null> {
    if (!this.canUseNativeMessageCache()) return null;
    try {
      const result = await invokeCommand<{ success?: boolean; messages?: any[]; meta?: MessageCacheMeta }>(
        'telegram_message_cache_get',
        { cacheKey },
      );
      if (result?.success) {
        return {
          messages: Array.isArray(result.messages) ? result.messages : [],
          meta: result.meta || {},
        };
      }
    } catch (error) {
      debugWarn('[MediaCacheService] Native message cache get failed:', error);
    }
    return null;
  }

  private async getNativeMessageMeta(cacheKey: string): Promise<MessageCacheMeta | null> {
    if (!this.canUseNativeMessageCache()) return null;
    try {
      return await invokeCommand<MessageCacheMeta>('telegram_message_cache_meta', { cacheKey });
    } catch (error) {
      debugWarn('[MediaCacheService] Native message cache meta failed:', error);
      return null;
    }
  }

  private async saveNativeMessages(cacheKey: string, messages: any[], meta?: MessageCacheMeta): Promise<boolean> {
    if (!this.canUseNativeMessageCache()) return false;
    try {
      const result = await invokeCommand<{ success?: boolean }>('telegram_message_cache_save', {
        cacheKey,
        messages,
        meta: meta || null,
      });
      return Boolean(result?.success);
    } catch (error) {
      debugWarn('[MediaCacheService] Native message cache save failed:', error);
      return false;
    }
  }

  async getSharedMediaMessages(cacheKey: string, limit = 12): Promise<any[]> {
    if (this.canUseNativeMessageCache()) {
      try {
        const result = await invokeCommand<{ success?: boolean; media?: any[] }>(
          'telegram_message_cache_shared_media',
          { cacheKey, limit },
        );
        if (result?.success && Array.isArray(result.media)) {
          return result.media;
        }
      } catch (error) {
        debugWarn('[MediaCacheService] Native shared media cache failed:', error);
      }
    }

    const messages = await this.getMessages(cacheKey);
    return messages
      .filter(message => message?.hasMedia)
      .slice(-limit)
      .map(message => ({
        id: message.id,
        isVideo: Boolean(message.isVideo),
        mediaSize: message.mediaSize ?? null,
      }));
  }

  private normalizeSegments(segments: MediaAssetSegment[] = []): MediaAssetSegment[] {
    return segments
      .map(segment => ({
        offset: Math.max(0, Number(segment.offset) || 0),
        length: Math.max(0, Number(segment.length) || 0),
      }))
      .filter(segment => segment.length > 0)
      .sort((a, b) => a.offset - b.offset || a.length - b.length);
  }

  private coverageBytes(segments: MediaAssetSegment[] = []) {
    const sorted = this.normalizeSegments(segments);
    let total = 0;
    let coverageStart = -1;
    let coverageEnd = -1;

    for (const segment of sorted) {
      const start = segment.offset;
      const end = segment.offset + segment.length;
      if (coverageStart < 0) {
        coverageStart = start;
        coverageEnd = end;
        continue;
      }
      if (start <= coverageEnd) {
        coverageEnd = Math.max(coverageEnd, end);
      } else {
        total += coverageEnd - coverageStart;
        coverageStart = start;
        coverageEnd = end;
      }
    }

    if (coverageStart >= 0) total += coverageEnd - coverageStart;
    return total;
  }

  async getMediaAssetMeta(key: string): Promise<MediaAssetMeta> {
    await this.ensureRegistryLoaded();

    try {
      const saved = await get<MediaAssetMeta>(this.mediaAssetMetaKey(key));
      if (saved) {
        return {
          ...saved,
          key,
          state: saved.state || 'empty',
          segments: this.normalizeSegments(saved.segments),
        };
      }
    } catch (e) {
      debugWarn("Failed to get media asset meta", e);
    }

    const info = this.registry.get(key);
    if (info?.nativeFilePath) {
      return {
        key,
        state: 'native',
        totalBytes: info.size || undefined,
        downloadedBytes: info.size || undefined,
        mimeType: info.mimeType,
        nativeFilePath: info.nativeFilePath,
        completedAt: info.addedAt,
        updatedAt: info.lastAccessed,
        segments: [],
      };
    }
    if (info) {
      return {
        key,
        state: 'complete',
        totalBytes: info.size,
        downloadedBytes: info.size,
        mimeType: info.mimeType,
        completedAt: info.addedAt,
        updatedAt: info.lastAccessed,
        segments: [],
      };
    }

    return { key, state: 'empty', downloadedBytes: 0, segments: [] };
  }

  async saveMediaAssetMeta(key: string, meta: Partial<MediaAssetMeta>): Promise<MediaAssetMeta> {
    const current = await this.getMediaAssetMeta(key);
    const segments = meta.segments
      ? this.normalizeSegments(meta.segments)
      : this.normalizeSegments(current.segments);
    const downloadedBytes = meta.downloadedBytes ?? this.coverageBytes(segments) ?? current.downloadedBytes;
    const updated: MediaAssetMeta = {
      ...current,
      ...meta,
      key,
      state: meta.state || current.state || 'empty',
      downloadedBytes,
      segments,
      updatedAt: Date.now(),
    };

    await set(this.mediaAssetMetaKey(key), updated);
    if (updated.state !== 'empty') {
      const existing = this.registry.get(key);
      this.registry.set(key, {
        key,
        size: updated.state === 'complete' || updated.state === 'native'
          ? Number(updated.totalBytes || updated.downloadedBytes || existing?.size || 0)
          : Number(updated.downloadedBytes || existing?.size || 0),
        mimeType: updated.mimeType || existing?.mimeType || 'application/octet-stream',
        type: 'media',
        addedAt: existing?.addedAt ?? Date.now(),
        lastAccessed: Date.now(),
        nativeFilePath: updated.nativeFilePath || existing?.nativeFilePath,
      });
      await this.saveRegistry();
    }
    return updated;
  }

  async saveMediaSegment(key: string, offset: number, buffer: ArrayBuffer | Uint8Array | any): Promise<MediaAssetMeta> {
    await this.ensureRegistryLoaded();
    const normalizedBuffer = this.toArrayBuffer(buffer);
    const normalizedOffset = Math.max(0, Number(offset) || 0);
    const length = normalizedBuffer.byteLength;
    if (length <= 0) return this.getMediaAssetMeta(key);

    await set(this.mediaSegmentKey(key, normalizedOffset, length), normalizedBuffer);
    const current = await this.getMediaAssetMeta(key);
    const segments = this.normalizeSegments([...(current.segments || []), { offset: normalizedOffset, length }]);
    const downloadedBytes = this.coverageBytes(segments);

    return this.saveMediaAssetMeta(key, {
      state: 'partial',
      downloadedBytes,
      segments,
    });
  }

  async getMediaSegment(key: string, offset: number, length: number): Promise<ArrayBuffer | null> {
    const full = await this.getMediaBuffer(key);
    const normalizedOffset = Math.max(0, Number(offset) || 0);
    const normalizedLength = Math.max(0, Number(length) || 0);
    if (normalizedLength <= 0) return new ArrayBuffer(0);
    if (full && normalizedOffset + normalizedLength <= full.byteLength) {
      return full.slice(normalizedOffset, normalizedOffset + normalizedLength);
    }

    const meta = await this.getMediaAssetMeta(key);
    const segments = this.normalizeSegments(meta.segments);
    let cursor = normalizedOffset;
    const end = normalizedOffset + normalizedLength;
    const parts: Uint8Array[] = [];

    for (const segment of segments) {
      const segmentStart = segment.offset;
      const segmentEnd = segment.offset + segment.length;
      if (segmentEnd <= cursor) continue;
      if (segmentStart > cursor) break;

      const segmentBuffer = await get<ArrayBuffer | Uint8Array | any>(this.mediaSegmentKey(key, segment.offset, segment.length));
      if (!segmentBuffer) break;
      const bytes = new Uint8Array(this.toArrayBuffer(segmentBuffer));
      const sliceStart = cursor - segmentStart;
      const sliceEnd = Math.min(bytes.byteLength, end - segmentStart);
      if (sliceEnd <= sliceStart) continue;

      parts.push(bytes.slice(sliceStart, sliceEnd));
      cursor += sliceEnd - sliceStart;
      if (cursor >= end) break;
    }

    if (cursor < end) return null;

    const merged = new Uint8Array(normalizedLength);
    let writeOffset = 0;
    for (const part of parts) {
      merged.set(part, writeOffset);
      writeOffset += part.byteLength;
    }
    return merged.buffer;
  }

  async finalizeMediaSegments(key: string, mimeType?: string): Promise<string | null> {
    const meta = await this.getMediaAssetMeta(key);
    const totalBytes = Number(meta.totalBytes || 0);
    if (totalBytes <= 0) return null;

    const fullBuffer = await this.getMediaSegment(key, 0, totalBytes);
    if (!fullBuffer || fullBuffer.byteLength < totalBytes) return null;

    const url = await this.saveMedia(key, fullBuffer, mimeType || meta.mimeType);
    await Promise.all(this.normalizeSegments(meta.segments).map(segment =>
      del(this.mediaSegmentKey(key, segment.offset, segment.length)).catch(() => {})
    ));
    await this.saveMediaAssetMeta(key, {
      state: 'complete',
      totalBytes,
      downloadedBytes: totalBytes,
      mimeType: mimeType || meta.mimeType,
      completedAt: Date.now(),
      segments: [],
    });
    return url;
  }

  // --- Message Cache ---
  async getMessages(chatId: string): Promise<any[]> {
    await this.ensureRegistryLoaded();

    const native = await this.getNativeMessages(chatId);
    if (native) {
      const key = `msgs_${chatId}`;
      const info = this.registry.get(key);
      if (info) {
        info.lastAccessed = Date.now();
        this.saveRegistry().catch(() => {});
      }
      return native.messages;
    }

    try {
      const msgs = await get<any[]>(`msgs_${chatId}`);
      if (msgs) {
        const key = `msgs_${chatId}`;
        const info = this.registry.get(key);
        if (info) {
          info.lastAccessed = Date.now();
          this.saveRegistry().catch(() => {});
        }
      }
      return msgs || [];
    } catch (e) {
      return [];
    }
  }

  async getMessagePageMeta(chatId: string): Promise<MessageCacheMeta> {
    const nativeMeta = await this.getNativeMessageMeta(chatId);
    if (nativeMeta) return nativeMeta;

    try {
      return await get<MessageCacheMeta>(`msgs_${chatId}_meta`) || {};
    } catch (e) {
      return {};
    }
  }

  async saveMessages(chatId: string, messages: any[], meta?: MessageCacheMeta): Promise<void> {
    await this.ensureRegistryLoaded();
    const nativeSaved = await this.saveNativeMessages(chatId, messages, meta);

    try {
      const key = `msgs_${chatId}`;
      if (!nativeSaved) {
        await set(key, messages);
      }
      if (meta) {
        const currentMeta = await this.getMessagePageMeta(chatId);
        if (!nativeSaved) {
          await set(`${key}_meta`, { ...currentMeta, ...meta });
        }
      }

      const size = JSON.stringify(messages).length;
      this.registry.set(key, {
        key,
        size,
        mimeType: 'application/json',
        type: 'message',
        addedAt: Date.now(),
        lastAccessed: Date.now()
      });
      await this.saveRegistry();
    } catch (e) {
      debugWarn("Error saving messages cache", e);
    }
  }

  /**
   * Explicitly cache an existing Blob URL in memory.
   */
  cacheUrlInMemory(key: string, url: string) {
    this.memoryCache.set(key, url);
  }

  /**
   * Clears the memory cache.
   */
  clearMemoryCache() {
    this.memoryCache.clear();
  }

  // --- Settings ---
  async getCacheSettings(): Promise<CacheSettings> {
    try {
      const settings = await get<CacheSettings>('cache_settings');
      if (settings) {
        return {
          maxCacheSize: settings.maxCacheSize ?? 0,
          avatarRefreshHours: settings.avatarRefreshHours ?? 24,
          downloadWorkers: settings.downloadWorkers ?? 4
        };
      }
    } catch (e) {}

    return {
      maxCacheSize: 0, // unlimited
      avatarRefreshHours: 24,
      downloadWorkers: 4
    };
  }

  async setCacheSettings(settings: Partial<CacheSettings>): Promise<boolean> {
    try {
      const current = await this.getCacheSettings();
      const updated = { ...current, ...settings };
      await set('cache_settings', updated);
      
      // Run eviction in case the new limit is lower
      await this.ensureRegistryLoaded();
      await this.evictIfNeeded();
      return true;
    } catch (e) {
      debugWarn("Failed to save cache settings", e);
      return false;
    }
  }

  // --- Stats ---
  async getCacheStats() {
    await this.ensureRegistryLoaded();
    
    let totalSize = 0;
    let messageCount = 0;
    let mediaCount = 0;
    let avatarCount = 0;

    this.registry.forEach(item => {
      totalSize += item.size;
      if (item.type === 'message') {
        messageCount++;
      } else if (item.type === 'media') {
        mediaCount++;
      } else if (item.type === 'avatar') {
        avatarCount++;
      }
    });

    return {
      success: true,
      totalSize,
      messageCount,
      mediaCount,
      avatarCount
    };
  }

  // --- Clear ---
  async clearCache(): Promise<void> {
    const settings = await this.getCacheSettings();
    
    // Completely clear the IndexedDB database
    await clear();
    
    // Reset local memory structures
    this.memoryCache.clear();
    this.registry.clear();
    
    // Re-save settings and registry
    await set('cache_settings', settings);
    await this.saveRegistry();
    debugLog("[MediaCacheService] Cache cleared successfully.");
  }

  // --- LRU Eviction ---
  private async evictIfNeeded(): Promise<void> {
    const settings = await this.getCacheSettings();
    if (settings.maxCacheSize === 0) return; // Unlimited

    let totalSize = 0;
    this.registry.forEach(item => {
      totalSize += item.size;
    });

    if (totalSize <= settings.maxCacheSize) return;

    // Evict oldest media items first
    const mediaItems = Array.from(this.registry.values())
      .filter(item => item.type === 'media')
      .sort((a, b) => a.lastAccessed - b.lastAccessed);

    debugLog(`[MediaCacheService] Eviction required: totalSize=${totalSize} bytes, limit=${settings.maxCacheSize} bytes.`);

    for (const item of mediaItems) {
      if (totalSize <= settings.maxCacheSize) break;

      debugLog(`[MediaCacheService] Evicting cached media: ${item.key} (size=${item.size} bytes)`);
      const meta = await this.getMediaAssetMeta(item.key);
      await Promise.all(this.normalizeSegments(meta.segments).map(segment =>
        del(this.mediaSegmentKey(item.key, segment.offset, segment.length)).catch(() => {})
      ));
      await del(this.mediaAssetMetaKey(item.key));
      await del(item.key);
      await del(`${item.key}_mime`);
      
      this.memoryCache.delete(item.key);
      this.registry.delete(item.key);
      
      totalSize -= item.size;
    }

    await this.saveRegistry();
  }
}

export const mediaCache = new MediaCacheService();
