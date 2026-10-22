import React, { useEffect, useState } from 'react';
import { IconX } from "../design-system/icons";
import { telegramService } from '../features/telegram/TelegramService';

export const IMAGE_MEDIA_MIME_TYPE = 'image/jpeg';

export const normalizeMediaBytes = (value?: number | null) => {
  const numeric = Number(value);
  return Number.isFinite(numeric) && numeric > 0 ? numeric : undefined;
};

export const formatMediaBytes = (bytes?: number) => {
  const numericBytes = normalizeMediaBytes(bytes);
  if (!numericBytes) return null;
  const units = ['B', 'KB', 'MB', 'GB'];
  const exponent = Math.min(Math.floor(Math.log(numericBytes) / Math.log(1024)), units.length - 1);
  const unit = units[exponent];
  if (!unit) return null;
  const value = numericBytes / Math.pow(1024, exponent);
  if (!Number.isFinite(value)) return null;
  return `${Number(value.toFixed(value >= 10 || exponent === 0 ? 0 : 1))} ${unit}`;
};

export const formatDuration = (seconds: number) => {
  const minutes = Math.floor(seconds / 60);
  const remainingSeconds = Math.floor(seconds % 60);
  return `${minutes}:${remainingSeconds.toString().padStart(2, '0')}`;
};

export const formatMessageTime = (dateNum?: number) => {
  if (!dateNum) return '';
  const date = new Date(dateNum * 1000);
  const hours = date.getHours().toString().padStart(2, '0');
  const minutes = date.getMinutes().toString().padStart(2, '0');
  return `${hours}:${minutes}`;
};

export const MediaSkeleton: React.FC<{ compact?: boolean }> = ({ compact = false }) => (
  <div className={`media-skeleton-shimmer ${compact ? 'compact' : ''}`} aria-hidden="true">
    <div className="media-skeleton-glow" />
  </div>
);

const miniThumbCache = new Map<string, string>();
const pendingMiniThumbs = new Map<string, Promise<string | null>>();

export const MessageMediaMini: React.FC<{ chatId: string; messageId: number }> = React.memo(({ chatId, messageId }) => {
  const cacheKey = `${chatId}_${messageId}`;
  const [thumb, setThumb] = useState<string | null>(() => miniThumbCache.get(cacheKey) ?? null);

  useEffect(() => {
    let isMounted = true;
    const cachedThumb = miniThumbCache.get(cacheKey);
    if (cachedThumb) {
      setThumb(cachedThumb);
      return () => { isMounted = false; };
    }

    const pendingThumb = pendingMiniThumbs.get(cacheKey)
      ?? telegramService.getMessageMedia({ chatId, messageId, priority: 'background' })
        .then(res => {
          const filePath = res.success && res.filePath ? res.filePath : null;
          if (filePath) miniThumbCache.set(cacheKey, filePath);
          return filePath;
        })
        .catch(() => null)
        .finally(() => {
          pendingMiniThumbs.delete(cacheKey);
        });

    pendingMiniThumbs.set(cacheKey, pendingThumb);
    pendingThumb.then(filePath => {
      if (isMounted && filePath) {
        setThumb(filePath);
      }
    });
    return () => { isMounted = false; };
  }, [cacheKey, chatId, messageId]);

  return (
    <div className="mini-thumb-container">
      {thumb ? (
        <img src={thumb} className="mini-thumb-img" alt="Thumbnail" />
      ) : (
        <MediaSkeleton compact />
      )}
    </div>
  );
});

export const MediaCancelControl: React.FC<{
  onCancel: (event: React.MouseEvent | React.KeyboardEvent) => void;
}> = ({ onCancel }) => (
  <span
    role="button"
    tabIndex={0}
    className="media-cancel-download-btn"
    title="Cancelar download"
    aria-label="Cancelar download da mídia"
    onClick={onCancel}
    onKeyDown={(event) => {
      if (event.key === 'Enter' || event.key === ' ') {
        onCancel(event);
      }
    }}
  >
    <IconX size={15} stroke={2} />
  </span>
);

export const MediaProgressBadge: React.FC<{
  label: string;
  value: string | number | null;
  video?: boolean;
}> = ({ label, value, video = false }) => (
  <div className={`media-progress-badge ${video ? 'media-progress-badge-video' : ''}`}>
    {label} {value}
  </div>
);
