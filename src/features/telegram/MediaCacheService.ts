import { get, set, del, clear } from 'idb-keyval';
import { debugLog, debugWarn } from '../../shared/debug/logger';

export interface CacheItemInfo {
  key: string;
  size: number;
  mimeType: string;
  type: 'avatar' | 'media' | 'message';
  addedAt: number;
  lastAccessed: number;
}

export interface CacheSettings {
  maxCacheSize: number;
  avatarRefreshHours: number;
  downloadWorkers: number;
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
      if (info) {
        info.lastAccessed = Date.now();
        this.saveRegistry().catch(() => {});
      }
      return this.memoryCache.get(key)!;
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
        await this.saveRegistry();

        return url;
      }
    } catch (e) {
      debugWarn("Failed to get media from IndexedDB cache", e);
    }

    return null;
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

    // Evict items if registry size exceeds the maximum limit
    this.evictIfNeeded().catch(e => debugWarn("Failed to evict cache", e));

    return url;
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

  // --- Message Cache ---
  async getMessages(chatId: string): Promise<any[]> {
    await this.ensureRegistryLoaded();

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

  async saveMessages(chatId: string, messages: any[]): Promise<void> {
    await this.ensureRegistryLoaded();

    try {
      const key = `msgs_${chatId}`;
      await set(key, messages);

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
