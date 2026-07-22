import React, { useEffect, useState, useRef } from 'react';
import { createPortal } from 'react-dom';
import { ArrowsInSimple, ArrowsOutSimple, Play, SpeakerSlash, SpeakerHigh, DownloadSimple, X } from "@phosphor-icons/react";
import { ContextMenu } from './ContextMenu';
import { telegramService } from '../features/telegram/TelegramService';
import { debugLog, debugWarn } from '../shared/debug/logger';

interface Props {
  chatId: string;
  messageId: number;
  isVideo: boolean;
  videoDuration?: number | null;
  messageDate?: number;
  mediaSize?: number | null;
  thumbnailUrl?: string | null;
  palette?: string;
  density?: string;
  onClickOverride?: (event?: React.MouseEvent) => void;
  selectionMode?: boolean;
  albumMedias?: Array<{ id: number; isVideo: boolean; videoDuration?: number | null; messageDate?: number; mediaSize?: number | null }>;
  downloadMeta?: Record<string, any>;
  mediaPriority?: 'visible' | 'background';
}

interface ContextMenuState {
  visible: boolean;
  x: number;
  y: number;
}

const IconDownload = () => <DownloadSimple size={18} weight="bold" />;
const IMAGE_MEDIA_MIME_TYPE = 'image/jpeg';

const getVideoDebugState = (video: HTMLVideoElement | null) => {
  if (!video) return null;
  const mp4Support = typeof video.canPlayType === 'function'
    ? video.canPlayType('video/mp4; codecs="avc1.42E01E, mp4a.40.2"')
    : '';
  const plainMp4Support = typeof video.canPlayType === 'function'
    ? video.canPlayType('video/mp4')
    : '';

  return {
    currentSrc: video.currentSrc,
    src: video.getAttribute('src'),
    networkState: video.networkState,
    readyState: video.readyState,
    errorCode: video.error?.code ?? null,
    errorMessage: video.error?.message ?? null,
    canPlayMp4: plainMp4Support,
    canPlayH264Aac: mp4Support,
    currentTime: video.currentTime,
    duration: Number.isFinite(video.duration) ? video.duration : null,
    paused: video.paused,
    muted: video.muted,
  };
};

const logVideoEvent = (label: string, video: HTMLVideoElement | null, context: Record<string, any>) => {
  debugLog(`[MessageMedia] ${label}`, {
    ...context,
    video: getVideoDebugState(video),
  });
};

const MediaSkeleton: React.FC<{ compact?: boolean }> = ({ compact = false }) => (
  <div className={`media-skeleton-shimmer ${compact ? 'compact' : ''}`} aria-hidden="true">
    <div className="media-skeleton-glow" />
  </div>
);

const MessageMediaMini: React.FC<{ chatId: string; messageId: number }> = ({ chatId, messageId }) => {
  const [thumb, setThumb] = useState<string | null>(null);

  useEffect(() => {
    let isMounted = true;
    telegramService.getMessageMedia({ chatId, messageId, priority: 'background' }).then(res => {
      if (isMounted && res.success && res.filePath) {
        setThumb(res.filePath);
      }
    });
    return () => { isMounted = false; };
  }, [chatId, messageId]);

  return (
    <div className="mini-thumb-container">
      {thumb ? (
        <img src={thumb} className="mini-thumb-img" alt="Thumbnail" />
      ) : (
        <MediaSkeleton compact />
      )}
    </div>
  );
};

export const MessageMedia: React.FC<Props> = ({ chatId, messageId, isVideo, videoDuration, messageDate, mediaSize, thumbnailUrl, palette, density, onClickOverride, selectionMode = false, albumMedias, downloadMeta, mediaPriority = 'visible' }) => {
  const [previewSrc, setPreviewSrc] = useState<string | null>(thumbnailUrl || null);
  const [loading, setLoading] = useState(true);
  const [isOpen, setIsOpen] = useState(false);
  const [savingMedia, setSavingMedia] = useState(false);
  const [cancelingMedia, setCancelingMedia] = useState(false);
  const fullMediaSrc = null;
  const loadingFullMedia = false;
  const [mediaProgress, setMediaProgress] = useState(0);
  const [mediaStage, setMediaStage] = useState<string | null>(null);
  const [mediaBytes, setMediaBytes] = useState<{ downloadedBytes?: number; totalBytes?: number }>({});
  const [contextMenu, setContextMenu] = useState<ContextMenuState>({ visible: false, x: 0, y: 0 });

  const [activeMessageId, setActiveMessageId] = useState(messageId);
  const [activeIsVideo, setActiveIsVideo] = useState(isVideo);

  const [activeFullSrc, setActiveFullSrc] = useState<string | null>(null);
  const [activeLoading, setActiveLoading] = useState(false);
  const [activeError, setActiveError] = useState<string | null>(null);
  const [playerProgress, setPlayerProgress] = useState(0);

  // Estados do Player Inline
  const [isInlinePlaying, setIsInlinePlaying] = useState(false);
  const [inlineStreamUrl, setInlineStreamUrl] = useState<string | null>(null);
  const [inlineLoading, setInlineLoading] = useState(false);
  const [inlineError, setInlineError] = useState<string | null>(null);
  const [isMuted, setIsMuted] = useState(true);
  const [inlineVideoProgress, setInlineVideoProgress] = useState(0);
  const inlineVideoRef = useRef<HTMLVideoElement>(null);
  const lightboxVideoShellRef = useRef<HTMLDivElement>(null);
  const [isPlayerFullscreen, setIsPlayerFullscreen] = useState(false);
  const [inlineBuffering, setInlineBuffering] = useState(false);
  const [lightboxBuffering, setLightboxBuffering] = useState(true);
  const [hasCachedFullMedia, setHasCachedFullMedia] = useState(false);
  const prefetchedFullMediaRef = useRef<Set<string>>(new Set());
  const activeLoadingRef = useRef(false);
  const inlineLoadingRef = useRef(false);
  const savingMediaRef = useRef(false);
  const fullPreviewLoadedRef = useRef(false);
  const prefetchTimersRef = useRef<Map<string, number>>(new Map());
  const canceledMediaRequestsRef = useRef<Set<string>>(new Set());

  const mediaRequestKey = (targetMessageId = messageId) => `${chatId}_${targetMessageId}`;

  useEffect(() => {
    const handleFullscreenChange = () => {
      setIsPlayerFullscreen(document.fullscreenElement === lightboxVideoShellRef.current);
    };
    document.addEventListener('fullscreenchange', handleFullscreenChange);
    return () => document.removeEventListener('fullscreenchange', handleFullscreenChange);
  }, []);

  const togglePlayerFullscreen = async (event: React.MouseEvent) => {
    event.stopPropagation();
    if (document.fullscreenElement) {
      await document.exitFullscreen();
      return;
    }
    await lightboxVideoShellRef.current?.requestFullscreen();
  };

  const clearDownloadVisualState = () => {
    setMediaStage(null);
    setMediaProgress(0);
    setMediaBytes({});
    setPlayerProgress(0);
    setInlineLoading(false);
    setInlineBuffering(false);
    setActiveLoading(false);
    setSavingMedia(false);
  };

  useEffect(() => {
    activeLoadingRef.current = activeLoading;
  }, [activeLoading]);

  useEffect(() => {
    inlineLoadingRef.current = inlineLoading;
  }, [inlineLoading]);

  useEffect(() => {
    savingMediaRef.current = savingMedia;
  }, [savingMedia]);

  const selectionInteractionLocked = selectionMode || Boolean(onClickOverride);

  useEffect(() => {
    if (!selectionInteractionLocked) return;

    inlineVideoRef.current?.pause();
    setIsInlinePlaying(false);
    setInlineStreamUrl(null);
    setInlineLoading(false);
    setInlineBuffering(false);
    setIsOpen(false);
    setActiveFullSrc(null);
    setActiveLoading(false);
    setActiveError(null);
    setContextMenu({ visible: false, x: 0, y: 0 });
  }, [selectionInteractionLocked]);

  useEffect(() => {
    let isMounted = true;
    fullPreviewLoadedRef.current = false;
    setHasCachedFullMedia(false);
    setPreviewSrc(thumbnailUrl || null);
    setLoading(!thumbnailUrl);

    const fetchMedia = async () => {
      const thumbRequest = telegramService.getMessageMedia({ chatId, messageId, priority: mediaPriority })
        .then(res => {
          if (!isMounted || !res.success || !res.filePath || fullPreviewLoadedRef.current) return;
          setPreviewSrc(res.filePath);
        })
        .catch(debugWarn)
        .finally(() => {
          if (isMounted) setLoading(false);
        });

      if (!isVideo) {
        telegramService.getCachedMessageMediaFile({ chatId, messageId, mimeType: IMAGE_MEDIA_MIME_TYPE })
          .then(cachedFull => {
            if (isMounted && cachedFull?.success && cachedFull.filePath) {
              fullPreviewLoadedRef.current = true;
              setPreviewSrc(cachedFull.filePath);
              setLoading(false);
            }
          })
          .catch(debugWarn);
      }

      try {
        if (thumbnailUrl) {
          await Promise.race([
            thumbRequest,
            new Promise(resolve => setTimeout(resolve, 1200)),
          ]);
        } else {
          await thumbRequest;
        }
      } catch (e) {
        debugWarn(e);
      }
    };

    fetchMedia();

    return () => {
      isMounted = false;
      if (mediaPriority === 'background') {
        telegramService.cancelBackgroundMessageMedia({ chatId, messageId });
      }
    };
  }, [chatId, messageId, isVideo, thumbnailUrl, mediaPriority]);

  useEffect(() => {
    return () => {
      for (const timer of prefetchTimersRef.current.values()) {
        window.clearTimeout(timer);
      }
      prefetchTimersRef.current.clear();
    };
  }, []);

  useEffect(() => {
    if (!isVideo) return;

    let isMounted = true;
    telegramService.isMessageMediaFileCached({ chatId, messageId, mimeType: 'video/mp4' })
      .then(isCached => {
        if (isMounted) setHasCachedFullMedia(isCached);
      })
      .catch(debugWarn);

    return () => {
      isMounted = false;
    };
  }, [chatId, messageId, isVideo]);

  useEffect(() => {
    const unsubscribe = telegramService.onMediaProgress((data: any) => {
      if (String(data.chatId) !== String(chatId) || Number(data.messageId) !== Number(messageId)) return;
      const key = mediaRequestKey(Number(data.messageId));
      if (data.stage === 'canceled') {
        canceledMediaRequestsRef.current.add(key);
        clearDownloadVisualState();
        setCancelingMedia(false);
        return;
      }
      if (canceledMediaRequestsRef.current.has(key)) {
        if (data.stage === 'ready') {
          canceledMediaRequestsRef.current.delete(key);
        } else {
          return;
        }
      }
      const progress = Math.max(0, Math.min(100, Number(data.progress) || 0));
      const downloadedBytes = Number(data.downloadedBytes);
      const totalBytes = Number(data.totalBytes);
      if (Number.isFinite(downloadedBytes) || Number.isFinite(totalBytes)) {
        setMediaBytes({
          downloadedBytes: Number.isFinite(downloadedBytes) ? downloadedBytes : undefined,
          totalBytes: Number.isFinite(totalBytes) ? totalBytes : undefined,
        });
      }
      if (activeLoadingRef.current || inlineLoadingRef.current) {
        setPlayerProgress(progress);
        setMediaStage(data.stage);
      }
      if (savingMediaRef.current || (!activeLoadingRef.current && !inlineLoadingRef.current)) {
        setMediaProgress(progress);
        setMediaStage(data.stage);
      }
      if (!isVideo && progress >= 100 && !fullPreviewLoadedRef.current) {
        void telegramService.getCachedMessageMediaFile({ chatId, messageId, mimeType: IMAGE_MEDIA_MIME_TYPE }).then((res: any) => {
          if (res?.success && res.filePath) {
            fullPreviewLoadedRef.current = true;
            setPreviewSrc(res.filePath);
          }
        }).catch(debugWarn);
      }
      if (isVideo && progress >= 100 && data.stage !== 'saving') {
        setHasCachedFullMedia(true);
      }
    });

    return () => {
      unsubscribe();
    };
  }, [chatId, messageId, isVideo]);

  useEffect(() => {
    if (albumMedias) {
      const activeItem = albumMedias.find(item => item.id === activeMessageId);
      if (activeItem) {
        setActiveIsVideo(activeItem.isVideo);
      }
    }
  }, [activeMessageId, albumMedias]);

  useEffect(() => {
    if (!isOpen) {
      setActiveFullSrc(null);
      setActiveError(null);
      setPlayerProgress(0);
      setMediaBytes({});
      return;
    }

    let isMounted = true;
    const loadActiveMedia = async () => {
      canceledMediaRequestsRef.current.delete(mediaRequestKey(activeMessageId));
      setActiveLoading(true);
      setActiveError(null);
      setPlayerProgress(0);
      setMediaBytes({});
      setLightboxBuffering(true);
      try {
        if (activeIsVideo) {
          const res = await telegramService.prepareMessageMediaPlayback({ chatId, messageId: activeMessageId, mode: 'lightbox' });
          debugLog(`[MessageMedia] Lightbox playback request result for ${chatId}/${activeMessageId}`, res);
          debugLog('[MessageMedia] Lightbox playback diagnostics', {
            chatId,
            messageId: activeMessageId,
            playbackUrl: res?.playbackUrl,
            filePath: res?.filePath,
            nativeFilePath: res?.nativeFilePath,
            mimeType: res?.mimeType,
            cacheState: res?.cacheState,
            totalBytes: res?.totalBytes,
            videoSupport: document.createElement('video').canPlayType(res?.mimeType || 'video/mp4'),
          });
          if (canceledMediaRequestsRef.current.has(mediaRequestKey(activeMessageId))) return;
          if (isMounted && res.success && res.playbackUrl) {
            setActiveFullSrc(res.playbackUrl);
            setPlayerProgress(100);
            setHasCachedFullMedia(res.cacheState === 'complete' || res.cacheState === 'native');
          } else if (isMounted && res?.canceled) {
            setActiveError(null);
          } else {
            debugWarn(`[MessageMedia] Lightbox playback request failed for ${chatId}/${activeMessageId}`, res);
            if (isMounted) setActiveError(res?.error || 'Não foi possível preparar o vídeo.');
          }
        } else {
          const res = await telegramService.getMessageMediaFile({ chatId, messageId: activeMessageId, mimeType: IMAGE_MEDIA_MIME_TYPE });
          if (canceledMediaRequestsRef.current.has(mediaRequestKey(activeMessageId))) return;
          if (isMounted && res.success && res.filePath) {
            setActiveFullSrc(res.filePath);
            setPlayerProgress(100);
          } else if (isMounted && res?.canceled) {
            setActiveError(null);
          } else if (isMounted) {
            setActiveError(res?.error || 'Não foi possível carregar a mídia.');
          }
        }
      } catch (err) {
        debugWarn(err);
        if (isMounted) setActiveError(err instanceof Error ? err.message : 'Não foi possível carregar a mídia.');
      } finally {
        if (isMounted) setActiveLoading(false);
      }
    };

    loadActiveMedia();

    return () => {
      isMounted = false;
    };
  }, [isOpen, activeMessageId, activeIsVideo, chatId]);

  useEffect(() => {
    if (!isOpen) return;

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        setIsOpen(false);
      } else if (e.key === 'ArrowRight' && albumMedias && albumMedias.length > 1) {
        e.preventDefault();
        const currentIndex = albumMedias.findIndex(item => item.id === activeMessageId);
        const nextIndex = (currentIndex + 1) % albumMedias.length;
        setActiveMessageId(albumMedias[nextIndex].id);
      } else if (e.key === 'ArrowLeft' && albumMedias && albumMedias.length > 1) {
        e.preventDefault();
        const currentIndex = albumMedias.findIndex(item => item.id === activeMessageId);
        const nextIndex = (currentIndex - 1 + albumMedias.length) % albumMedias.length;
        setActiveMessageId(albumMedias[nextIndex].id);
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => {
      window.removeEventListener('keydown', handleKeyDown);
    };
  }, [isOpen, activeMessageId, albumMedias]);

  if (loading) {
    return (
      <div className={`media-preview media-skeleton ${isVideo ? 'is-video' : 'is-image'}`}>
        <MediaSkeleton />
      </div>
    );
  }

  const handleSelectionClick = (event?: React.MouseEvent) => {
    if (!selectionInteractionLocked) return false;
    event?.preventDefault();
    event?.stopPropagation();
    inlineVideoRef.current?.pause();
    setIsInlinePlaying(false);
    setInlineStreamUrl(null);
    setInlineLoading(false);
    setInlineBuffering(false);
    setIsOpen(false);
    onClickOverride?.(event);
    return true;
  };

  const handleOpen = (event?: React.MouseEvent) => {
    if (handleSelectionClick(event)) return;
    inlineVideoRef.current?.pause();
    setActiveMessageId(messageId);
    setIsOpen(true);
  };

  const handleInlinePlay = async (e: React.MouseEvent) => {
    if (handleSelectionClick(e)) return;
    e.stopPropagation();
    
    if (inlineStreamUrl) {
      debugLog(`[MessageMedia] Reusing inline stream URL for ${chatId}/${messageId}: ${inlineStreamUrl}`);
      setIsInlinePlaying(true);
      if (inlineVideoRef.current) {
        inlineVideoRef.current.play().catch(err => {
          debugWarn(`[MessageMedia] Inline video play() failed for ${chatId}/${messageId}`, err, getVideoDebugState(inlineVideoRef.current));
        });
      }
      return;
    }
    
    setInlineLoading(true);
    setInlineBuffering(true);
    setInlineError(null);
    setPlayerProgress(0);
    setMediaBytes({});
    canceledMediaRequestsRef.current.delete(mediaRequestKey(messageId));
    try {
      const res = await telegramService.prepareMessageMediaPlayback({ chatId, messageId, mode: 'inline' });
      debugLog(`[MessageMedia] Inline stream request result for ${chatId}/${messageId}`, res);
      debugLog('[MessageMedia] Inline playback diagnostics', {
        chatId,
        messageId,
        playbackUrl: res?.playbackUrl,
        filePath: res?.filePath,
        nativeFilePath: res?.nativeFilePath,
        mimeType: res?.mimeType,
        cacheState: res?.cacheState,
        totalBytes: res?.totalBytes,
        videoSupport: document.createElement('video').canPlayType(res?.mimeType || 'video/mp4'),
      });
      if (canceledMediaRequestsRef.current.has(mediaRequestKey(messageId))) return;
      if (res.success && res.playbackUrl) {
        setInlineStreamUrl(res.playbackUrl);
        setHasCachedFullMedia(res.cacheState === 'complete' || res.cacheState === 'native');
        setIsInlinePlaying(true);
        setPlayerProgress(100);
      } else if (res?.canceled) {
        setInlineError(null);
        setInlineBuffering(false);
      } else {
        debugWarn(`[MessageMedia] Inline stream request failed for ${chatId}/${messageId}`, res);
        setInlineError(res?.error || 'Não foi possível iniciar o vídeo.');
        setInlineBuffering(false);
      }
    } catch (err) {
      debugWarn(`[MessageMedia] Inline stream request threw for ${chatId}/${messageId}`, err);
      setInlineError(err instanceof Error ? err.message : 'Não foi possível iniciar o vídeo.');
      setInlineBuffering(false);
    } finally {
      setInlineLoading(false);
    }
  };

  const handleTimeUpdate = () => {
    if (inlineVideoRef.current && videoDuration) {
      setInlineVideoProgress((inlineVideoRef.current.currentTime / videoDuration) * 100);
    }
  };

  const handleSaveMedia = async (saveAs = false) => {
    if (savingMedia) return;

    setSavingMedia(true);
    setMediaStage('downloading');
    setMediaProgress(0);
    canceledMediaRequestsRef.current.delete(mediaRequestKey(activeMessageId));

    try {
      const res = await telegramService.saveMessageMediaFile({ chatId, messageId: activeMessageId, downloadMeta, saveAs });
      if (canceledMediaRequestsRef.current.has(mediaRequestKey(activeMessageId))) return;
      if (res?.canceled) {
        return;
      }
      if (!res?.success) {
        debugWarn('[MessageMedia] Failed to save media', res);
      }
    } catch (e) {
      debugWarn(e);
    } finally {
      setMediaStage(null);
      setMediaProgress(0);
      setMediaBytes({});
      setSavingMedia(false);
    }
  };

  const handleContextMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setContextMenu({ visible: true, x: e.clientX, y: e.clientY });
  };

  const handleCancelMediaDownload = async (event?: React.MouseEvent | React.KeyboardEvent, targetMessageId = activeMessageId || messageId) => {
    event?.preventDefault();
    event?.stopPropagation();
    if (cancelingMedia) return;

    canceledMediaRequestsRef.current.add(mediaRequestKey(targetMessageId));
    setCancelingMedia(true);
    inlineVideoRef.current?.pause();
    setIsInlinePlaying(false);
    setInlineStreamUrl(null);
    clearDownloadVisualState();

    try {
      await telegramService.cancelMessageMediaDownload({ chatId, messageId: targetMessageId });
    } catch (error) {
      debugWarn(error);
    } finally {
      setCancelingMedia(false);
    }
  };

  const scheduleFullMediaPrefetch = (targetMessageId: number) => {
    const key = `${chatId}_${targetMessageId}`;
    if (prefetchedFullMediaRef.current.has(key)) return;
    prefetchedFullMediaRef.current.add(key);

    const timer = window.setTimeout(() => {
      prefetchTimersRef.current.delete(key);
      telegramService.prefetchMessageMediaFile({ chatId, messageId: targetMessageId });
    }, 1800);
    prefetchTimersRef.current.set(key, timer);
  };

  const closeContextMenu = () => {
    setContextMenu((prev) => ({ ...prev, visible: false }));
  };

  const contextMenuItems = [
    {
      label: 'Salvar como...',
      icon: <IconDownload />,
      onClick: () => handleSaveMedia(true),
      disabled: savingMedia,
    },
    ...(albumMedias && albumMedias.length > 1 ? [{
      label: 'Salvar álbum como...',
      icon: <IconDownload />,
      onClick: async () => {
        const folderResult = await telegramService.selectFolder();
        if (!folderResult.success || !folderResult.folderPath) return;
        await telegramService.saveMultipleMediaFiles({
          chatId,
          messageIds: albumMedias.map(item => item.id),
          folderPath: folderResult.folderPath,
        });
      },
      disabled: savingMedia,
    }] : []),
  ];

  const canOpenViewer = Boolean(previewSrc || fullMediaSrc || isVideo);
  const isSavingInBackground = savingMedia || (mediaStage === 'saving' || mediaStage === 'downloading') && mediaProgress > 0 && mediaProgress < 100;
  const shouldShowProgress = loadingFullMedia || isSavingInBackground;
  const visiblePlayerProgress = playerProgress > 0 && playerProgress < 100 ? playerProgress : mediaProgress;
  const normalizeBytes = (value?: number | null) => {
    const numeric = Number(value);
    return Number.isFinite(numeric) && numeric > 0 ? numeric : undefined;
  };
  const activeMediaSize = normalizeBytes(albumMedias?.find(item => item.id === activeMessageId)?.mediaSize ?? mediaSize);
  const knownTotalBytes = normalizeBytes(mediaBytes.totalBytes) || activeMediaSize;
  const knownDownloadedBytes = mediaBytes.downloadedBytes
    ?? (knownTotalBytes && visiblePlayerProgress > 0 ? Math.round((visiblePlayerProgress / 100) * knownTotalBytes) : undefined);
  const formatMediaBytes = (bytes?: number) => {
    const numericBytes = normalizeBytes(bytes);
    if (!numericBytes) return null;
    const units = ['B', 'KB', 'MB', 'GB'];
    const exponent = Math.min(Math.floor(Math.log(numericBytes) / Math.log(1024)), units.length - 1);
    const unit = units[exponent];
    if (!unit) return null;
    const value = numericBytes / Math.pow(1024, exponent);
    if (!Number.isFinite(value)) return null;
    return `${Number(value.toFixed(value >= 10 || exponent === 0 ? 0 : 1))} ${units[exponent]}`;
  };
  const progressBytesLabel = knownTotalBytes
    ? `${formatMediaBytes(knownDownloadedBytes) || '0 B'} / ${formatMediaBytes(knownTotalBytes)}`
    : formatMediaBytes(knownDownloadedBytes);
  const progressDetailLabel = progressBytesLabel || (visiblePlayerProgress > 0 && visiblePlayerProgress < 100 ? `${visiblePlayerProgress}%` : null);
  let progressLabel = 'Preparando';
  if (savingMedia || mediaStage === 'downloading') {
    progressLabel = 'Baixando';
  } else if (mediaStage === 'saving') {
    progressLabel = 'Salvando';
  }
  const mediaSizeLabel = formatMediaBytes(normalizeBytes(mediaSize));
  const shouldShowVideoSizeChip = Boolean(isVideo && mediaSizeLabel && !isInlinePlaying && !hasCachedFullMedia);
  const hasCancelableMediaProgress = (
    mediaStage === 'downloading'
    || mediaStage === 'saving'
  ) && visiblePlayerProgress > 0 && visiblePlayerProgress < 100;
  const canCancelMediaDownload = !cancelingMedia && (savingMedia || isSavingInBackground || hasCancelableMediaProgress);
  const renderCancelMediaControl = (targetMessageId = activeMessageId || messageId) => {
    if (!canCancelMediaDownload) return null;
    return (
      <span
        role="button"
        tabIndex={0}
        className="media-cancel-download-btn"
        title="Cancelar download"
        aria-label="Cancelar download da mídia"
        onClick={(event) => handleCancelMediaDownload(event, targetMessageId)}
        onKeyDown={(event) => {
          if (event.key === 'Enter' || event.key === ' ') {
            handleCancelMediaDownload(event, targetMessageId);
          }
        }}
      >
        <X size={15} weight="bold" />
      </span>
    );
  };

  const formatDuration = (seconds: number) => {
    const m = Math.floor(seconds / 60);
    const s = Math.floor(seconds % 60);
    return `${m}:${s.toString().padStart(2, '0')}`;
  };

  const formatMessageTime = (dateNum?: number) => {
    if (!dateNum) return '';
    const d = new Date(dateNum * 1000);
    const h = d.getHours().toString().padStart(2, '0');
    const m = d.getMinutes().toString().padStart(2, '0');
    return `${h}:${m}`;
  };

  const shouldRenderInlinePlayer = isInlinePlaying && inlineStreamUrl && !selectionInteractionLocked;

  if (previewSrc || fullMediaSrc || isVideo) {
    return (
      <>
      <div className={`media-preview ${isVideo ? 'is-video' : 'is-image'} ${isInlinePlaying ? 'playing-inline' : ''}`}>
        {shouldRenderInlinePlayer ? (
          <div className="inline-video-wrapper" onClick={handleOpen}>
            <video
              ref={inlineVideoRef}
              className="inline-video-player"
              src={inlineStreamUrl}
              autoPlay
              muted={isMuted}
              playsInline
              onLoadStart={event => logVideoEvent('inline loadstart', event.currentTarget, { chatId, messageId, src: inlineStreamUrl })}
              onLoadedMetadata={event => logVideoEvent('inline loadedmetadata', event.currentTarget, { chatId, messageId, src: inlineStreamUrl })}
              onLoadedData={event => logVideoEvent('inline loadeddata', event.currentTarget, { chatId, messageId, src: inlineStreamUrl })}
              onProgress={event => logVideoEvent('inline progress', event.currentTarget, { chatId, messageId, src: inlineStreamUrl })}
              onSuspend={event => logVideoEvent('inline suspend', event.currentTarget, { chatId, messageId, src: inlineStreamUrl })}
              onAbort={event => logVideoEvent('inline abort', event.currentTarget, { chatId, messageId, src: inlineStreamUrl })}
              onCanPlay={event => {
                setInlineBuffering(false);
                logVideoEvent('inline canplay', event.currentTarget, { chatId, messageId, src: inlineStreamUrl });
              }}
              onPlaying={event => {
                setInlineBuffering(false);
                scheduleFullMediaPrefetch(messageId);
                logVideoEvent('inline playing', event.currentTarget, { chatId, messageId, src: inlineStreamUrl });
              }}
              onWaiting={event => {
                setInlineBuffering(true);
                logVideoEvent('inline waiting', event.currentTarget, { chatId, messageId, src: inlineStreamUrl });
              }}
              onStalled={event => {
                setInlineBuffering(true);
                logVideoEvent('inline stalled', event.currentTarget, { chatId, messageId, src: inlineStreamUrl });
              }}
              onError={event => {
                logVideoEvent('inline error', event.currentTarget, { chatId, messageId, src: inlineStreamUrl });
                setInlineBuffering(false);
                setInlineError('Falha ao reproduzir vídeo.');
                setIsInlinePlaying(false);
                setInlineStreamUrl(null);
              }}
              onTimeUpdate={handleTimeUpdate}
            />
            <button 
              className="inline-mute-btn" 
              onClick={(e) => {
                e.stopPropagation();
                setIsMuted(!isMuted);
              }}
            >
              {isMuted ? <SpeakerSlash size={16} weight="fill" color="white" /> : <SpeakerHigh size={16} weight="fill" color="white" />}
            </button>
            {!inlineBuffering && !shouldShowProgress && (
              <div className="inline-progress-bar">
                <div className="inline-progress-fill" style={{ width: `${inlineVideoProgress}%` }}></div>
              </div>
            )}
            {(shouldShowProgress || inlineBuffering) && (
              <div className="video-play-icon loading">
                <span className="spinner"></span>
              </div>
            )}
            {shouldShowProgress && (
              <div className="media-progress-badge media-progress-badge-video">
                {progressLabel} {progressBytesLabel || `${mediaProgress}%`}
              </div>
            )}
            {renderCancelMediaControl(messageId)}
          </div>
        ) : (
          <button
            type="button"
            className="media-preview-button"
            onClick={(e) => {
              if (handleSelectionClick(e)) return;
              if (isVideo) {
                handleInlinePlay(e);
              } else if (canOpenViewer) {
                handleOpen(e);
              }
            }}
            disabled={!selectionInteractionLocked && !canOpenViewer}
          >
            {previewSrc ? (
              <img
                src={previewSrc}
                alt="Media"
                className="media-img"
                onContextMenu={handleContextMenu}
              />
            ) : (
              <div className="media-preview media-skeleton" onContextMenu={handleContextMenu} style={{ border: 'none', width: '100%', height: '100%' }}>
                <MediaSkeleton compact={inlineLoading || shouldShowProgress} />
              </div>
            )}
            
            {shouldShowProgress && (
              isVideo ? (
                <>
                  <div className="video-play-icon loading">
                    <span className="spinner"></span>
                  </div>
                  <div className="media-progress-badge media-progress-badge-video">
                    {progressLabel} {progressBytesLabel || `${mediaProgress}%`}
                  </div>
                </>
              ) : (
                <div className="media-progress-badge">
                  {progressLabel} {progressBytesLabel || `${mediaProgress}%`}
                </div>
              )
            )}
            
            {inlineLoading && !shouldShowProgress && (
              <div className="video-play-icon loading">
                <span className="spinner"></span>
              </div>
            )}

            {renderCancelMediaControl(messageId)}

            {inlineError && !inlineLoading && !shouldShowProgress && (
              <div className="media-status-chip error">
                {inlineError}
              </div>
            )}

            {previewSrc && isVideo && !shouldShowProgress && !inlineLoading && (
              <div className="video-play-icon">
                <Play size={24} weight="fill" color="white" />
              </div>
            )}
            
            {previewSrc && shouldShowVideoSizeChip && (
              <div className="media-overlay-pill top-left media-size-overlay">
                {mediaSizeLabel}
              </div>
            )}

            {previewSrc && isVideo && videoDuration != null && (
              <div className="media-overlay-pill top-right">
                {formatDuration(videoDuration)}
              </div>
            )}
            
            {previewSrc && !isVideo && messageDate != null && (
              <div className="media-overlay-pill bottom-right">
                {formatMessageTime(messageDate)}
              </div>
            )}
          </button>
        )}
      </div>

        {contextMenu.visible && (
          <ContextMenu
            x={contextMenu.x}
            y={contextMenu.y}
            items={contextMenuItems}
            onClose={closeContextMenu}
            palette={palette}
            density={density}
          />
        )}

        {isOpen && createPortal(
          <div
            className="media-lightbox"
            data-palette={palette}
            data-density={density}
            onClick={() => setIsOpen(false)}
          >
            <div className="media-lightbox-toolbar" onClick={event => event.stopPropagation()}>
              <button
                type="button"
                className="btn-icon"
                onClick={(event) => savingMedia ? handleCancelMediaDownload(event, activeMessageId) : handleSaveMedia(true)}
                disabled={cancelingMedia}
                title={savingMedia ? 'Cancelar download' : 'Download'}
                aria-label={savingMedia ? 'Cancelar download da mídia' : 'Salvar mídia'}
              >
                {savingMedia ? <X size={20} weight="bold" /> : <DownloadSimple size={20} weight="bold" />}
              </button>
              <button
                type="button"
                className="btn-icon"
                onClick={() => setIsOpen(false)}
                aria-label="Fechar visualizacao de midia"
              >
                ×
              </button>
            </div>
            <div
              className={`media-lightbox-content ${activeIsVideo ? 'video-content' : 'image-content'}`}
            >
              {activeLoading || (!activeFullSrc && !activeError) ? (
                <div className="media-lightbox-loading" onClick={event => event.stopPropagation()}>
                  <div className="media-lightbox-preparing">
                    <span className="spinner"></span>
                    {progressDetailLabel && (
                      <div className="media-lightbox-progress-track">
                        <div
                          className="media-lightbox-progress-fill"
                          style={{ width: visiblePlayerProgress > 0 && visiblePlayerProgress < 100 ? `${visiblePlayerProgress}%` : '42%' }}
                        />
                      </div>
                    )}
                    {renderCancelMediaControl(activeMessageId)}
                  </div>
                </div>
              ) : activeIsVideo && activeFullSrc && !activeError ? (
                <div className="media-video-shell" ref={lightboxVideoShellRef}>
                  <video
                    className="media-video-player"
                    src={activeFullSrc}
                    poster={previewSrc || undefined}
                    controls
                    controlsList="nofullscreen"
                    autoPlay
                    playsInline
                    preload="auto"
                    onLoadStart={event => logVideoEvent('lightbox loadstart', event.currentTarget, { chatId, messageId: activeMessageId, src: activeFullSrc })}
                    onLoadedMetadata={event => {
                      setLightboxBuffering(false);
                      logVideoEvent('lightbox loadedmetadata', event.currentTarget, { chatId, messageId: activeMessageId, src: activeFullSrc });
                    }}
                    onLoadedData={event => logVideoEvent('lightbox loadeddata', event.currentTarget, { chatId, messageId: activeMessageId, src: activeFullSrc })}
                    onProgress={event => logVideoEvent('lightbox progress', event.currentTarget, { chatId, messageId: activeMessageId, src: activeFullSrc })}
                    onSuspend={event => logVideoEvent('lightbox suspend', event.currentTarget, { chatId, messageId: activeMessageId, src: activeFullSrc })}
                    onAbort={event => logVideoEvent('lightbox abort', event.currentTarget, { chatId, messageId: activeMessageId, src: activeFullSrc })}
                    onCanPlay={event => {
                      setLightboxBuffering(false);
                      logVideoEvent('lightbox canplay', event.currentTarget, { chatId, messageId: activeMessageId, src: activeFullSrc });
                    }}
                    onPlaying={event => {
                      setLightboxBuffering(false);
                      scheduleFullMediaPrefetch(activeMessageId);
                      logVideoEvent('lightbox playing', event.currentTarget, { chatId, messageId: activeMessageId, src: activeFullSrc });
                    }}
                    onWaiting={event => {
                      setLightboxBuffering(true);
                      logVideoEvent('lightbox waiting', event.currentTarget, { chatId, messageId: activeMessageId, src: activeFullSrc });
                    }}
                    onSeeking={event => {
                      setLightboxBuffering(true);
                      logVideoEvent('lightbox seeking', event.currentTarget, { chatId, messageId: activeMessageId, src: activeFullSrc });
                    }}
                    onSeeked={() => setLightboxBuffering(false)}
                    onStalled={event => {
                      setLightboxBuffering(true);
                      logVideoEvent('lightbox stalled', event.currentTarget, { chatId, messageId: activeMessageId, src: activeFullSrc });
                    }}
                    onError={event => {
                      logVideoEvent('lightbox error', event.currentTarget, { chatId, messageId: activeMessageId, src: activeFullSrc });
                      setLightboxBuffering(false);
                      setActiveError('Falha ao reproduzir vídeo.');
                      setActiveFullSrc(null);
                    }}
                    onContextMenu={handleContextMenu}
                    onClick={event => event.stopPropagation()}
                  />
                  <button
                    type="button"
                    className="media-player-fullscreen-btn"
                    onClick={togglePlayerFullscreen}
                    aria-label={isPlayerFullscreen ? 'Sair da tela cheia' : 'Entrar em tela cheia'}
                    title={isPlayerFullscreen ? 'Sair da tela cheia' : 'Tela cheia'}
                  >
                    {isPlayerFullscreen ? <ArrowsInSimple size={20} weight="bold" /> : <ArrowsOutSimple size={20} weight="bold" />}
                  </button>
                  {(lightboxBuffering || shouldShowProgress) && (
                    <div 
                      className={`video-play-icon loading ${shouldShowProgress ? 'progress-loading' : ''}`}
                      onClick={event => event.stopPropagation()}
                    >
                      <span className="spinner"></span>
                      {shouldShowProgress && (
                        <span className="loading-text">{progressLabel} {progressBytesLabel || `${mediaProgress}%`}</span>
                      )}
                    </div>
                  )}
                </div>
              ) : activeFullSrc ? (
                <img
                  src={activeFullSrc}
                  alt="Media expandida"
                  className="media-lightbox-img"
                  onContextMenu={handleContextMenu}
                  onClick={event => event.stopPropagation()}
                />
              ) : (
                <div className="media-preview failed media-lightbox-failed" onClick={event => event.stopPropagation()}>
                  <strong>Mídia indisponível</strong>
                  {activeError && <span>{activeError}</span>}
                </div>
              )}
            </div>

            {albumMedias && albumMedias.length > 1 && (
              <div className="media-lightbox-carousel" onClick={e => e.stopPropagation()}>
                {albumMedias.map(item => (
                  <button
                    key={item.id}
                    className={`carousel-thumb ${item.id === activeMessageId ? 'active' : ''}`}
                    onClick={() => setActiveMessageId(item.id)}
                  >
                    <MessageMediaMini chatId={chatId} messageId={item.id} />
                  </button>
                ))}
              </div>
            )}

            {contextMenu.visible && (
            <ContextMenu
              x={contextMenu.x}
              y={contextMenu.y}
              items={contextMenuItems}
              onClose={closeContextMenu}
              palette={palette}
              density={density}
            />
            )}
          </div>,
          document.querySelector('.dashboard-container') || document.body
        )}
      </>
    );
  }

  return (
    <div className={`media-preview media-skeleton ${isVideo ? 'is-video' : 'is-image'}`}>
      <MediaSkeleton />
    </div>
  );
};
