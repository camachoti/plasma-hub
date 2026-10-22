import { useEffect, useRef, useState } from 'react';
import { IconAlertCircle, IconCircleCheck, IconCircleX, IconFileDownload, IconStack2 } from '../../design-system/icons';
import type { DownloadItem } from './DownloadService';
import { downloadService } from './DownloadService';
import { runtimeCapabilities } from '../../shared/platform/runtime';
import { convertFileSrc, invokeCommand } from '../../shared/platform/tauri';
import '../../styles/DownloadArtwork.css';

const imageFile = /\.(?:avif|bmp|gif|jpe?g|png|webp)$/i;
const videoFile = /\.(?:avi|m4v|mkv|mov|mp4|mpeg|mpg|webm)$/i;
const thumbnailRequests = new Map<string, Promise<string | null>>();
let thumbnailQueue: Promise<unknown> = Promise.resolve();

function telegramMessageId(item: DownloadItem): number | null {
  if (Number.isSafeInteger(item.messageId) && item.messageId! > 0) return item.messageId!;
  const match = /^media_(\d+)\.(?:jpe?g|png|mp4|mov|webm)$/i.exec(item.fileName);
  const inferred = match ? Number(match[1]) : NaN;
  return Number.isSafeInteger(inferred) && inferred > 0 ? inferred : null;
}

function nativeTelegramArtwork(item: DownloadItem): Promise<string | null> {
  const messageId = telegramMessageId(item);
  if (!runtimeCapabilities.supportsTdlib || !item.chatId || !/^-?\d+$/.test(item.chatId) || !messageId) return Promise.resolve(null);
  const key = `telegram:${item.chatId}:${messageId}`;
  let request = thumbnailRequests.get(key);
  if (!request) {
    request = thumbnailQueue.then(() => invokeCommand<{ success: boolean; filePath?: string | null }>(
      'tdlib_download_message_thumbnail',
      { chatId: Number(item.chatId), messageId },
    ))
      .then(result => result.success && result.filePath ? convertFileSrc(result.filePath) : null)
      .catch(() => null)
      .then(url => {
        if (!url) thumbnailRequests.delete(key);
        return url;
      });
    thumbnailRequests.set(key, request);
    thumbnailQueue = request;
  }
  return request;
}

async function localArtwork(item: DownloadItem, failedSource: string | null): Promise<string | null> {
  const path = item.filePath;
  if (path && runtimeCapabilities.isTauri && item.status === 'completed') {
    if (imageFile.test(path)) {
      const imageUrl = convertFileSrc(path);
      if (imageUrl !== failedSource) return imageUrl;
    } else if (videoFile.test(path) && !runtimeCapabilities.isAndroid) {
      let request = thumbnailRequests.get(path);
      if (!request) {
        request = thumbnailQueue.then(() => invokeCommand<string>('generate_video_thumbnail', { filePath: path }))
          .then(thumbnailPath => thumbnailPath ? convertFileSrc(thumbnailPath) : null)
          .catch(() => null)
          .then(url => {
            if (!url) thumbnailRequests.delete(path);
            return url;
          });
        thumbnailRequests.set(path, request);
        thumbnailQueue = request;
      }
      const videoUrl = await request;
      if (videoUrl && videoUrl !== failedSource) return videoUrl;
    }
  }
  const telegramUrl = await nativeTelegramArtwork(item);
  return telegramUrl !== failedSource ? telegramUrl : null;
}

function StatusIcon({ status, group }: { status: DownloadItem['status']; group?: boolean }) {
  if (group) return <IconStack2 size={20} stroke={2} />;
  if (status === 'completed') return <IconCircleCheck size={20} stroke={2} />;
  if (status === 'failed') return <IconAlertCircle size={20} stroke={2} />;
  if (status === 'canceled') return <IconCircleX size={20} stroke={2} />;
  return <IconFileDownload size={20} stroke={2} />;
}

export function DownloadArtwork({ item, group = false, status = item.status }: { item: DownloadItem; group?: boolean; status?: DownloadItem['status'] }) {
  const [source, setSource] = useState(item.thumbnailUrl || null);
  const [visible, setVisible] = useState(false);
  const [failedSource, setFailedSource] = useState<string | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (item.thumbnailUrl && item.thumbnailUrl !== failedSource) setSource(item.thumbnailUrl);
  }, [item.thumbnailUrl, failedSource]);

  useEffect(() => {
    const element = containerRef.current;
    if (!element) return;
    if (!('IntersectionObserver' in window)) {
      setVisible(true);
      return;
    }
    const observer = new IntersectionObserver(entries => {
      if (entries.some(entry => entry.isIntersecting)) {
        setVisible(true);
        observer.disconnect();
      }
    }, { rootMargin: '160px' });
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    if (!visible || source || item.platform !== 'telegram') return;
    let cancelled = false;
    void localArtwork(item, failedSource).then(url => {
      if (cancelled || !url || url === failedSource) return;
      setSource(url);
      if (url !== item.thumbnailUrl) downloadService.updateDownload(item.id, { thumbnailUrl: url });
    });
    return () => { cancelled = true; };
  }, [failedSource, item.chatId, item.fileName, item.filePath, item.id, item.messageId, item.platform, item.status, item.thumbnailUrl, source, visible]);

  return (
    <div ref={containerRef} className="download-icon-wrapper">
      {source ? (
        <div className="download-thumbnail-container">
          <img
            src={source}
            alt={`Prévia de ${item.fileName}`}
            className="download-thumbnail-img"
            loading="lazy"
            onError={() => { setFailedSource(source); setSource(null); }}
          />
          <div className={`download-status-overlay ${status}`}><StatusIcon status={status} /></div>
        </div>
      ) : (
        <div className={`download-icon ${status}`}><StatusIcon status={status} group={group} /></div>
      )}
    </div>
  );
}
