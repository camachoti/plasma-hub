import React, { useEffect, useState, useRef } from 'react';
import { DownloadSimple } from "@phosphor-icons/react";
import { ContextMenu } from './ContextMenu';
import { MessageMediaLightbox } from './MessageMediaLightbox';
import { MessageMediaPreview } from './MessageMediaPreview';
import { useMessageMediaProgress } from './useMessageMediaProgress';
import { telegramService } from '../features/telegram/TelegramService';
import { debugLog, debugWarn } from '../shared/debug/logger';
import {
  IMAGE_MEDIA_MIME_TYPE,
  MediaCancelControl,
  MediaSkeleton,
} from './MessageMediaPrimitives';

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

const areAlbumMediasEqual = (left?: Props['albumMedias'], right?: Props['albumMedias']) => {
  if (left === right) return true;
  if (!left || !right || left.length !== right.length) return false;
  return left.every((item, index) => {
    const other = right[index];
    return item.id === other.id
      && item.isVideo === other.isVideo
      && item.videoDuration === other.videoDuration
      && item.messageDate === other.messageDate
      && item.mediaSize === other.mediaSize;
  });
};

const areDownloadMetaEqual = (left?: Props['downloadMeta'], right?: Props['downloadMeta']) => {
  if (left === right) return true;
  if (!left || !right) return false;
  return left.chatTitle === right.chatTitle
    && left.chatKind === right.chatKind
    && left.topicTitle === right.topicTitle
    && left.senderName === right.senderName
    && left.senderId === right.senderId;
};

const areMessageMediaPropsEqual = (prev: Props, next: Props) => (
  prev.chatId === next.chatId
  && prev.messageId === next.messageId
  && prev.isVideo === next.isVideo
  && prev.videoDuration === next.videoDuration
  && prev.messageDate === next.messageDate
  && prev.mediaSize === next.mediaSize
  && prev.thumbnailUrl === next.thumbnailUrl
  && prev.palette === next.palette
  && prev.density === next.density
  && prev.selectionMode === next.selectionMode
  && prev.mediaPriority === next.mediaPriority
  && Boolean(prev.onClickOverride) === Boolean(next.onClickOverride)
  && areAlbumMediasEqual(prev.albumMedias, next.albumMedias)
  && areDownloadMetaEqual(prev.downloadMeta, next.downloadMeta)
);

const MessageMediaComponent: React.FC<Props> = ({ chatId, messageId, isVideo, videoDuration, messageDate, mediaSize, thumbnailUrl, palette, density, onClickOverride, selectionMode = false, albumMedias, downloadMeta, mediaPriority = 'visible' }) => {
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
  const thumbnailFallbackRequestedRef = useRef(false);
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
    thumbnailFallbackRequestedRef.current = false;
    setHasCachedFullMedia(false);
    setPreviewSrc(thumbnailUrl || null);
    setLoading(!thumbnailUrl);

    const fetchMedia = async () => {
      const thumbRequest = thumbnailUrl
        ? Promise.resolve()
        : telegramService.getMessageMedia({ chatId, messageId, priority: mediaPriority })
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
        await thumbRequest;
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

  const handlePreviewImageError = () => {
    if (thumbnailFallbackRequestedRef.current) return;
    thumbnailFallbackRequestedRef.current = true;
    telegramService.getMessageMedia({ chatId, messageId, priority: 'visible' })
      .then(res => {
        if (res?.success && res.filePath) setPreviewSrc(res.filePath);
      })
      .catch(debugWarn);
  };

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
    let progressFrameId: number | null = null;
    let pendingProgressData: any = null;

    const applyProgressUpdate = (data: any) => {
      const key = mediaRequestKey(Number(data.messageId));
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
    };

    const unsubscribe = telegramService.onMessageMediaProgress(chatId, messageId, (data: any) => {
      const key = mediaRequestKey(Number(data.messageId));
      if (data.stage === 'canceled') {
        if (progressFrameId !== null) {
          window.cancelAnimationFrame(progressFrameId);
          progressFrameId = null;
        }
        pendingProgressData = null;
        canceledMediaRequestsRef.current.add(key);
        clearDownloadVisualState();
        setCancelingMedia(false);
        return;
      }

      pendingProgressData = data;
      if (progressFrameId !== null) return;

      progressFrameId = window.requestAnimationFrame(() => {
        progressFrameId = null;
        const latestProgressData = pendingProgressData;
        pendingProgressData = null;
        if (latestProgressData) applyProgressUpdate(latestProgressData);
      });
    });

    return () => {
      if (progressFrameId !== null) {
        window.cancelAnimationFrame(progressFrameId);
      }
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
  const {
    canCancelMediaDownload,
    mediaSizeLabel,
    progressBytesLabel,
    progressDetailLabel,
    progressLabel,
    shouldShowProgress,
    shouldShowVideoSizeChip,
    visiblePlayerProgress,
  } = useMessageMediaProgress({
    activeMessageId,
    albumMedias,
    mediaSize,
    mediaBytes,
    mediaProgress,
    mediaStage,
    playerProgress,
    loadingFullMedia,
    savingMedia,
    cancelingMedia,
    isVideo,
    isInlinePlaying,
    hasCachedFullMedia,
  });
  const renderCancelMediaControl = (targetMessageId = activeMessageId || messageId) => {
    if (!canCancelMediaDownload) return null;
    return (
      <MediaCancelControl onCancel={(event) => handleCancelMediaDownload(event, targetMessageId)} />
    );
  };

  const shouldRenderInlinePlayer = isInlinePlaying && inlineStreamUrl && !selectionInteractionLocked;

  if (previewSrc || fullMediaSrc || isVideo) {
    return (
      <>
      <MessageMediaPreview
        chatId={chatId}
        messageId={messageId}
        isVideo={isVideo}
        videoDuration={videoDuration}
        messageDate={messageDate}
        previewSrc={previewSrc}
        inlineStreamUrl={inlineStreamUrl}
        shouldRenderInlinePlayer={Boolean(shouldRenderInlinePlayer)}
        shouldShowProgress={shouldShowProgress}
        inlineBuffering={inlineBuffering}
        inlineLoading={inlineLoading}
        inlineError={inlineError}
        inlineVideoProgress={inlineVideoProgress}
        isMuted={isMuted}
        canOpenViewer={canOpenViewer}
        selectionInteractionLocked={selectionInteractionLocked}
        progressLabel={progressLabel}
        progressBytesLabel={progressBytesLabel}
        mediaProgress={mediaProgress}
        shouldShowVideoSizeChip={shouldShowVideoSizeChip}
        hasCachedFullMedia={hasCachedFullMedia}
        mediaSizeLabel={mediaSizeLabel}
        inlineVideoRef={inlineVideoRef}
        onOpen={handleOpen}
        onInlinePlay={handleInlinePlay}
        onSelectionClick={handleSelectionClick}
        onContextMenu={handleContextMenu}
        onPreviewImageError={handlePreviewImageError}
        onSetMuted={setIsMuted}
        onSetInlineBuffering={setInlineBuffering}
        onSetInlineError={setInlineError}
        onSetInlinePlaying={setIsInlinePlaying}
        onSetInlineStreamUrl={setInlineStreamUrl}
        onTimeUpdate={handleTimeUpdate}
        onCancelControl={renderCancelMediaControl}
        onScheduleFullMediaPrefetch={scheduleFullMediaPrefetch}
        onLogVideoEvent={logVideoEvent}
      />

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

        {isOpen && (
          <MessageMediaLightbox
            palette={palette}
            density={density}
            chatId={chatId}
            activeMessageId={activeMessageId}
            activeIsVideo={activeIsVideo}
            activeFullSrc={activeFullSrc}
            activeError={activeError}
            activeLoading={activeLoading}
            previewSrc={previewSrc}
            savingMedia={savingMedia}
            cancelingMedia={cancelingMedia}
            lightboxBuffering={lightboxBuffering}
            shouldShowProgress={shouldShowProgress}
            progressLabel={progressLabel}
            progressBytesLabel={progressBytesLabel}
            mediaProgress={mediaProgress}
            visiblePlayerProgress={visiblePlayerProgress}
            progressDetailLabel={progressDetailLabel}
            isPlayerFullscreen={isPlayerFullscreen}
            albumMedias={albumMedias}
            contextMenu={contextMenu}
            contextMenuItems={contextMenuItems}
            videoShellRef={lightboxVideoShellRef}
            onClose={() => setIsOpen(false)}
            onSave={(event) => savingMedia ? handleCancelMediaDownload(event, activeMessageId) : handleSaveMedia(true)}
            onToggleFullscreen={togglePlayerFullscreen}
            onContextMenu={handleContextMenu}
            onCancelControl={renderCancelMediaControl}
            onSelectAlbumMedia={setActiveMessageId}
            onSetLightboxBuffering={setLightboxBuffering}
            onSetActiveError={setActiveError}
            onSetActiveFullSrc={setActiveFullSrc}
            onScheduleFullMediaPrefetch={scheduleFullMediaPrefetch}
            onCloseContextMenu={closeContextMenu}
            onLogVideoEvent={logVideoEvent}
          />
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

export const MessageMedia = React.memo(MessageMediaComponent, areMessageMediaPropsEqual);
