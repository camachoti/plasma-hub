import React, { useEffect, useState, useRef } from 'react';
import { createPortal } from 'react-dom';
import { Play, SpeakerSlash, SpeakerHigh, ImageSquare, FilmStrip, DownloadSimple } from "@phosphor-icons/react";
import { ContextMenu } from './ContextMenu';
import { telegramService } from '../features/telegram/TelegramService';

interface Props {
  chatId: string;
  messageId: number;
  isVideo: boolean;
  videoDuration?: number | null;
  messageDate?: number;
  mediaSize?: number | null;
  palette?: string;
  density?: string;
  onClickOverride?: () => void;
  albumMedias?: Array<{ id: number; isVideo: boolean; videoDuration?: number | null; messageDate?: number; mediaSize?: number | null }>;
  downloadMeta?: Record<string, any>;
}

interface ContextMenuState {
  visible: boolean;
  x: number;
  y: number;
}

const IconDownload = () => <DownloadSimple size={18} weight="bold" />;
const IconPhotoPlaceholder = () => <ImageSquare size={32} weight="fill" color="var(--text-3)" style={{ opacity: 0.6 }} />;
const IconVideoPlaceholder = () => <FilmStrip size={32} weight="fill" color="var(--text-3)" style={{ opacity: 0.6 }} />;

const getVideoDebugState = (video: HTMLVideoElement | null) => {
  if (!video) return null;

  return {
    currentSrc: video.currentSrc,
    networkState: video.networkState,
    readyState: video.readyState,
    errorCode: video.error?.code ?? null,
    errorMessage: video.error?.message ?? null,
    currentTime: video.currentTime,
    duration: Number.isFinite(video.duration) ? video.duration : null,
    paused: video.paused,
    muted: video.muted,
  };
};

const logVideoEvent = (label: string, video: HTMLVideoElement | null, context: Record<string, any>) => {
  console.log(`[MessageMedia] ${label}`, {
    ...context,
    video: getVideoDebugState(video),
  });
};

const MessageMediaMini: React.FC<{ chatId: string; messageId: number; isVideo: boolean }> = ({ chatId, messageId, isVideo }) => {
  const [thumb, setThumb] = useState<string | null>(null);

  useEffect(() => {
    let isMounted = true;
    telegramService.getMessageMedia({ chatId, messageId }).then(res => {
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
        <div className="mini-thumb-placeholder">
        {isVideo ? <Play size={12} weight="fill" color="white" /> : <ImageSquare size={12} weight="fill" color="white" />}
      </div>
      )}
    </div>
  );
};

export const MessageMedia: React.FC<Props> = ({ chatId, messageId, isVideo, videoDuration, messageDate, palette, density, onClickOverride, albumMedias, downloadMeta }) => {
  const [previewSrc, setPreviewSrc] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [isOpen, setIsOpen] = useState(false);
  const [savingMedia, setSavingMedia] = useState(false);
  const fullMediaSrc = null;
  const loadingFullMedia = false;
  const [mediaProgress, setMediaProgress] = useState(0);
  const [mediaStage, setMediaStage] = useState<string | null>(null);
  const [contextMenu, setContextMenu] = useState<ContextMenuState>({ visible: false, x: 0, y: 0 });

  const [activeMessageId, setActiveMessageId] = useState(messageId);
  const [activeIsVideo, setActiveIsVideo] = useState(isVideo);

  const [activeFullSrc, setActiveFullSrc] = useState<string | null>(null);
  const [activeStreamUrl, setActiveStreamUrl] = useState<string | null>(null);
  const [activeLoading, setActiveLoading] = useState(false);

  // Estados do Player Inline
  const [isInlinePlaying, setIsInlinePlaying] = useState(false);
  const [inlineStreamUrl, setInlineStreamUrl] = useState<string | null>(null);
  const [inlineLoading, setInlineLoading] = useState(false);
  const [isMuted, setIsMuted] = useState(true);
  const [inlineVideoProgress, setInlineVideoProgress] = useState(0);
  const inlineVideoRef = useRef<HTMLVideoElement>(null);
  const [inlineBuffering, setInlineBuffering] = useState(false);
  const [lightboxBuffering, setLightboxBuffering] = useState(true);

  useEffect(() => {
    let isMounted = true;

    const fetchMedia = async () => {
      try {
        const res = await telegramService.getMessageMedia({ chatId, messageId });

        if (!isMounted || !res.success) return;

        if (res.filePath) {
          setPreviewSrc(res.filePath);
        }
      } catch (e) {
        console.error(e);
      } finally {
        if (isMounted) setLoading(false);
      }
    };

    fetchMedia();

    return () => {
      isMounted = false;
    };
  }, [chatId, messageId]);

  useEffect(() => {
    const unsubscribe = telegramService.onMediaProgress((data: any) => {
      if (data.chatId !== chatId || data.messageId !== messageId) return;
      setMediaProgress(data.progress);
      setMediaStage(data.stage);
    });

    return () => {
      unsubscribe();
    };
  }, [chatId, messageId]);

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
      setActiveStreamUrl(null);
      return;
    }

    let isMounted = true;
    const loadActiveMedia = async () => {
      setActiveLoading(true);
      setLightboxBuffering(true);
      setMediaStage('downloading');
      setMediaProgress(0);
      try {
        if (activeIsVideo) {
          setMediaStage('streaming');
          const res = await telegramService.getMessageMediaStream({ chatId, messageId: activeMessageId });
          console.log(`[MessageMedia] Lightbox stream request result for ${chatId}/${activeMessageId}`, res);
          if (isMounted && res.success && res.streamUrl) {
            setActiveStreamUrl(res.streamUrl);
            setActiveFullSrc(null);
          } else {
            console.warn(`[MessageMedia] Lightbox stream request failed for ${chatId}/${activeMessageId}`, res);
          }
        } else {
          const res = await telegramService.getMessageMediaFile({ chatId, messageId: activeMessageId });
          if (isMounted && res.success && res.filePath) {
            setActiveFullSrc(res.filePath);
            setActiveStreamUrl(null);
          }
        }
      } catch (err) {
        console.error(err);
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
        <div className="media-skeleton-inner">
          {isVideo ? <IconVideoPlaceholder /> : <IconPhotoPlaceholder />}
          <span className="media-skeleton-text">
            {isVideo ? 'Carregando Vídeo' : 'Carregando Imagem'}
          </span>
        </div>
      </div>
    );
  }

  const handleOpen = () => {
    setActiveMessageId(messageId);
    setIsOpen(true);
  };

  const handleInlinePlay = async (e: React.MouseEvent) => {
    e.stopPropagation();
    
    if (inlineStreamUrl) {
      console.log(`[MessageMedia] Reusing inline stream URL for ${chatId}/${messageId}: ${inlineStreamUrl}`);
      setIsInlinePlaying(true);
      if (inlineVideoRef.current) {
        inlineVideoRef.current.play().catch(err => {
          console.error(`[MessageMedia] Inline video play() failed for ${chatId}/${messageId}`, err, getVideoDebugState(inlineVideoRef.current));
        });
      }
      return;
    }
    
    setInlineLoading(true);
    setMediaStage('streaming');
    try {
      const res = await telegramService.getMessageMediaStream({ chatId, messageId });
      console.log(`[MessageMedia] Inline stream request result for ${chatId}/${messageId}`, res);
      if (res.success && res.streamUrl) {
        setInlineStreamUrl(res.streamUrl);
        setIsInlinePlaying(true);
      } else {
        console.warn(`[MessageMedia] Inline stream request failed for ${chatId}/${messageId}`, res);
      }
    } catch (err) {
      console.error(`[MessageMedia] Inline stream request threw for ${chatId}/${messageId}`, err);
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

    try {
      await telegramService.saveMessageMediaFile({ chatId, messageId: activeMessageId, downloadMeta, saveAs });
    } catch (e) {
      console.error(e);
    } finally {
      setSavingMedia(false);
    }
  };

  const handleContextMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setContextMenu({ visible: true, x: e.clientX, y: e.clientY });
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
  ];

  const canOpenViewer = Boolean(previewSrc || fullMediaSrc || isVideo);
  const shouldShowProgress = loadingFullMedia || savingMedia || (mediaProgress > 0 && mediaProgress < 100);
  let progressLabel = 'Carregando';
  if (savingMedia || mediaStage === 'downloading') {
    progressLabel = 'Baixando';
  } else if (mediaStage === 'streaming') {
    progressLabel = 'Streaming';
  }

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

  if (previewSrc || fullMediaSrc || isVideo) {
    return (
      <>
      <div className={`media-preview ${isVideo ? 'is-video' : 'is-image'} ${isInlinePlaying ? 'playing-inline' : ''}`}>
        {isInlinePlaying && inlineStreamUrl ? (
          <div className="inline-video-wrapper" onClick={handleOpen}>
            <video
              ref={inlineVideoRef}
              className="inline-video-player"
              src={inlineStreamUrl}
              autoPlay
              muted={isMuted}
              playsInline
              onLoadedMetadata={event => logVideoEvent('inline loadedmetadata', event.currentTarget, { chatId, messageId, src: inlineStreamUrl })}
              onCanPlay={event => {
                setInlineBuffering(false);
                logVideoEvent('inline canplay', event.currentTarget, { chatId, messageId, src: inlineStreamUrl });
              }}
              onPlaying={event => {
                setInlineBuffering(false);
                logVideoEvent('inline playing', event.currentTarget, { chatId, messageId, src: inlineStreamUrl });
              }}
              onWaiting={event => logVideoEvent('inline waiting', event.currentTarget, { chatId, messageId, src: inlineStreamUrl })}
              onStalled={event => logVideoEvent('inline stalled', event.currentTarget, { chatId, messageId, src: inlineStreamUrl })}
              onError={event => logVideoEvent('inline error', event.currentTarget, { chatId, messageId, src: inlineStreamUrl })}
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
            <div className="inline-progress-bar">
              <div className="inline-progress-fill" style={{ width: `${inlineVideoProgress}%` }}></div>
            </div>
            {(shouldShowProgress || inlineBuffering) && (
              <div className={`video-play-icon loading ${shouldShowProgress ? 'progress-loading' : ''}`}>
                <span className="spinner"></span>
                {shouldShowProgress && (
                  <span className="loading-text">{mediaProgress}%</span>
                )}
              </div>
            )}
          </div>
        ) : (
          <button
            type="button"
            className="media-preview-button"
            onClick={(e) => {
              if (onClickOverride) {
                onClickOverride();
              } else if (isVideo) {
                handleInlinePlay(e);
              } else if (canOpenViewer) {
                handleOpen();
              }
            }}
            disabled={!onClickOverride && !canOpenViewer}
          >
            {previewSrc ? (
              <img
                src={previewSrc}
                alt="Media"
                className="media-img"
                onContextMenu={handleContextMenu}
              />
            ) : (
              <div className="media-preview failed" onContextMenu={handleContextMenu} style={{ border: 'none', background: 'none', width: '100%', height: '100%' }}>
                Video
              </div>
            )}
            
            {shouldShowProgress && (
              isVideo ? (
                <div className="video-play-icon loading progress-loading">
                  <span className="spinner"></span>
                  <span className="loading-text">{mediaProgress}%</span>
                </div>
              ) : (
                <div className="media-progress-badge">
                  {progressLabel} {mediaProgress}%
                </div>
              )
            )}
            
            {inlineLoading && !shouldShowProgress && (
              <div className="video-play-icon loading">
                <span className="spinner"></span>
              </div>
            )}

            {isVideo && !shouldShowProgress && !inlineLoading && (
              <div className="video-play-icon">
                <Play size={24} weight="fill" color="white" />
              </div>
            )}
            
            {isVideo && videoDuration != null && (
              <div className="media-overlay-pill top-left">
                {formatDuration(videoDuration)}
              </div>
            )}
            
            {messageDate != null && (
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
                onClick={() => handleSaveMedia(false)}
                disabled={savingMedia}
                title={savingMedia ? 'Salvando...' : 'Download'}
                aria-label="Salvar mídia"
              >
                {savingMedia ? <span className="spinner small-spinner"></span> : <DownloadSimple size={20} weight="bold" />}
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
              {activeLoading ? (
                <div className="media-lightbox-loading" onClick={event => event.stopPropagation()}>
                  <div className="media-lightbox-progress">
                    <span className="spinner"></span>
                    <span>{progressLabel} {mediaProgress}%</span>
                  </div>
                </div>
              ) : activeIsVideo && activeStreamUrl ? (
                <div className="media-video-shell">
                  <video
                    className="media-video-player"
                    src={activeStreamUrl}
                    controls={!lightboxBuffering}
                    autoPlay
                    playsInline
                    preload="auto"
                    onLoadedMetadata={event => logVideoEvent('lightbox loadedmetadata', event.currentTarget, { chatId, messageId: activeMessageId, src: activeStreamUrl })}
                    onCanPlay={event => {
                      setLightboxBuffering(false);
                      logVideoEvent('lightbox canplay', event.currentTarget, { chatId, messageId: activeMessageId, src: activeStreamUrl });
                    }}
                    onPlaying={event => {
                      setLightboxBuffering(false);
                      logVideoEvent('lightbox playing', event.currentTarget, { chatId, messageId: activeMessageId, src: activeStreamUrl });
                    }}
                    onWaiting={event => logVideoEvent('lightbox waiting', event.currentTarget, { chatId, messageId: activeMessageId, src: activeStreamUrl })}
                    onSeeking={event => logVideoEvent('lightbox seeking', event.currentTarget, { chatId, messageId: activeMessageId, src: activeStreamUrl })}
                    onSeeked={() => setLightboxBuffering(false)}
                    onStalled={event => logVideoEvent('lightbox stalled', event.currentTarget, { chatId, messageId: activeMessageId, src: activeStreamUrl })}
                    onError={event => logVideoEvent('lightbox error', event.currentTarget, { chatId, messageId: activeMessageId, src: activeStreamUrl })}
                    onContextMenu={handleContextMenu}
                    onClick={event => event.stopPropagation()}
                  />
                  {(lightboxBuffering || shouldShowProgress) && (
                    <div 
                      className={`video-play-icon loading ${shouldShowProgress ? 'progress-loading' : ''}`}
                      onClick={event => event.stopPropagation()}
                    >
                      <span className="spinner"></span>
                      {shouldShowProgress && (
                        <span className="loading-text">{mediaProgress}%</span>
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
                <div className="media-preview failed" onClick={event => event.stopPropagation()}>Midia indisponivel</div>
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
                    <MessageMediaMini chatId={chatId} messageId={item.id} isVideo={item.isVideo} />
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

  return <div className="media-preview failed">[Media]</div>;
};
