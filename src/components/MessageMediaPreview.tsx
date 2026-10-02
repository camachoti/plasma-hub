import React, { useEffect, useState } from 'react';
import { LoadingIndicator } from '../design-system';
import { IconPlayerPlay, IconVolume, IconVolumeOff } from "../design-system/icons";
import {
  formatDuration,
  MediaProgressBadge,
  MediaSkeleton,
} from './MessageMediaPrimitives';

interface MessageMediaPreviewProps {
  chatId: string;
  messageId: number;
  isVideo: boolean;
  videoDuration?: number | null;
  messageDate?: number;
  previewSrc: string | null;
  inlineStreamUrl: string | null;
  shouldRenderInlinePlayer: boolean;
  shouldShowProgress: boolean;
  inlineBuffering: boolean;
  inlineLoading: boolean;
  inlineError: string | null;
  inlineVideoProgress: number;
  isMuted: boolean;
  canOpenViewer: boolean;
  selectionInteractionLocked: boolean;
  progressLabel: string;
  progressBytesLabel: string | null;
  mediaProgress: number;
  shouldShowVideoSizeChip: boolean;
  hasCachedFullMedia: boolean;
  mediaSizeLabel: string | null;
  inlineVideoRef: React.RefObject<HTMLVideoElement | null>;
  onOpen: (event?: React.MouseEvent) => void;
  onInlinePlay: (event: React.MouseEvent) => void;
  onSelectionClick: (event?: React.MouseEvent) => boolean;
  onContextMenu: (event: React.MouseEvent) => void;
  onPreviewImageError: () => void;
  onSetMuted: (muted: boolean) => void;
  onSetInlineBuffering: (buffering: boolean) => void;
  onSetInlineError: (error: string | null) => void;
  onSetInlinePlaying: (playing: boolean) => void;
  onSetInlineStreamUrl: (url: string | null) => void;
  onTimeUpdate: () => void;
  onCancelControl: (targetMessageId?: number) => React.ReactNode;
  onScheduleFullMediaPrefetch: (messageId: number) => void;
  onLogVideoEvent: (label: string, video: HTMLVideoElement | null, context: Record<string, any>) => void;
}

type MediaShape = 'unknown' | 'landscape' | 'portrait' | 'square';

export const MessageMediaPreview: React.FC<MessageMediaPreviewProps> = ({
  chatId,
  messageId,
  isVideo,
  videoDuration,
  previewSrc,
  inlineStreamUrl,
  shouldRenderInlinePlayer,
  shouldShowProgress,
  inlineBuffering,
  inlineLoading,
  inlineError,
  inlineVideoProgress,
  isMuted,
  canOpenViewer,
  selectionInteractionLocked,
  progressLabel,
  progressBytesLabel,
  mediaProgress,
  shouldShowVideoSizeChip,
  hasCachedFullMedia,
  mediaSizeLabel,
  inlineVideoRef,
  onOpen,
  onInlinePlay,
  onSelectionClick,
  onContextMenu,
  onPreviewImageError,
  onSetMuted,
  onSetInlineBuffering,
  onSetInlineError,
  onSetInlinePlaying,
  onSetInlineStreamUrl,
  onTimeUpdate,
  onCancelControl,
  onScheduleFullMediaPrefetch,
  onLogVideoEvent,
}) => {
  const [mediaShape, setMediaShape] = useState<MediaShape>('unknown');
  const [mediaRatio, setMediaRatio] = useState<string | null>(null);

  useEffect(() => {
    setMediaShape('unknown');
    setMediaRatio(null);
  }, [messageId, previewSrc]);

  const updateMediaShape = (width: number, height: number) => {
    if (!width || !height) return;

    const rawRatio = width / height;
    const boundedRatio = Math.min(16 / 9, Math.max(3 / 4, rawRatio));
    setMediaShape(rawRatio > 1.12 ? 'landscape' : rawRatio < 0.88 ? 'portrait' : 'square');
    setMediaRatio(`${boundedRatio} / 1`);
  };

  const previewStyle = mediaRatio
    ? ({ '--media-preview-ratio': mediaRatio } as React.CSSProperties)
    : undefined;

  return (
  <div
    className={`media-preview media-shape-${mediaShape} ${isVideo ? 'is-video' : 'is-image'} ${shouldRenderInlinePlayer ? 'playing-inline' : ''}`}
    style={previewStyle}
  >
    {shouldRenderInlinePlayer ? (
      <div className="inline-video-wrapper" onClick={onOpen}>
        <video
          ref={inlineVideoRef}
          className="inline-video-player"
          src={inlineStreamUrl || undefined}
          autoPlay
          muted={isMuted}
          playsInline
          onLoadStart={event => onLogVideoEvent('inline loadstart', event.currentTarget, { chatId, messageId, src: inlineStreamUrl })}
          onLoadedMetadata={event => {
            updateMediaShape(event.currentTarget.videoWidth, event.currentTarget.videoHeight);
            onLogVideoEvent('inline loadedmetadata', event.currentTarget, { chatId, messageId, src: inlineStreamUrl });
          }}
          onLoadedData={event => onLogVideoEvent('inline loadeddata', event.currentTarget, { chatId, messageId, src: inlineStreamUrl })}
          onProgress={event => onLogVideoEvent('inline progress', event.currentTarget, { chatId, messageId, src: inlineStreamUrl })}
          onSuspend={event => onLogVideoEvent('inline suspend', event.currentTarget, { chatId, messageId, src: inlineStreamUrl })}
          onAbort={event => onLogVideoEvent('inline abort', event.currentTarget, { chatId, messageId, src: inlineStreamUrl })}
          onCanPlay={event => {
            onSetInlineBuffering(false);
            onLogVideoEvent('inline canplay', event.currentTarget, { chatId, messageId, src: inlineStreamUrl });
          }}
          onPlaying={event => {
            onSetInlineBuffering(false);
            onScheduleFullMediaPrefetch(messageId);
            onLogVideoEvent('inline playing', event.currentTarget, { chatId, messageId, src: inlineStreamUrl });
          }}
          onWaiting={event => {
            onSetInlineBuffering(true);
            onLogVideoEvent('inline waiting', event.currentTarget, { chatId, messageId, src: inlineStreamUrl });
          }}
          onStalled={event => {
            onSetInlineBuffering(true);
            onLogVideoEvent('inline stalled', event.currentTarget, { chatId, messageId, src: inlineStreamUrl });
          }}
          onError={event => {
            onLogVideoEvent('inline error', event.currentTarget, { chatId, messageId, src: inlineStreamUrl });
            onSetInlineBuffering(false);
            onSetInlineError('Falha ao reproduzir vídeo.');
            onSetInlinePlaying(false);
            onSetInlineStreamUrl(null);
          }}
          onTimeUpdate={onTimeUpdate}
        />
        <button
          type="button"
          className="inline-mute-btn"
          aria-label={isMuted ? 'Ativar som do vídeo' : 'Silenciar vídeo'}
          aria-pressed={!isMuted}
          onClick={(event) => {
            event.stopPropagation();
            onSetMuted(!isMuted);
          }}
        >
          {isMuted ? <IconVolumeOff size={16} stroke={2} color="white" /> : <IconVolume size={16} stroke={2} color="white" />}
        </button>
        {!inlineBuffering && !shouldShowProgress && (
          <div className="inline-progress-bar">
            <div className="inline-progress-fill" style={{ width: `${inlineVideoProgress}%` }}></div>
          </div>
        )}
        {(shouldShowProgress || inlineBuffering) && (
          <div className="video-play-icon loading">
            <LoadingIndicator size="sm" className="loading-indicator--light" />
          </div>
        )}
        {shouldShowProgress && (
          <MediaProgressBadge label={progressLabel} value={progressBytesLabel || `${mediaProgress}%`} video />
        )}
        {onCancelControl(messageId)}
      </div>
    ) : (
      <button
        type="button"
        className="media-preview-button"
        aria-label={isVideo ? 'Reproduzir vídeo' : 'Abrir imagem'}
        onClick={(event) => {
          if (onSelectionClick(event)) return;
          if (isVideo) {
            onInlinePlay(event);
          } else if (canOpenViewer) {
            onOpen(event);
          }
        }}
        disabled={!selectionInteractionLocked && !canOpenViewer}
      >
        {previewSrc ? (
          <img
            src={previewSrc}
            alt="Media"
            className="media-img"
            onContextMenu={onContextMenu}
            onError={onPreviewImageError}
            onLoad={event => updateMediaShape(event.currentTarget.naturalWidth, event.currentTarget.naturalHeight)}
          />
        ) : isVideo ? (
          <div className="video-preview-unavailable" onContextMenu={onContextMenu}>
            <span className="video-preview-unavailable-icon"><IconPlayerPlay size={20} stroke={2} /></span>
            <span>Prévia indisponível</span>
          </div>
        ) : (
          <div className="media-preview media-skeleton" onContextMenu={onContextMenu} style={{ border: 'none', width: '100%', height: '100%' }}>
            <MediaSkeleton compact={inlineLoading || shouldShowProgress} />
          </div>
        )}

        {shouldShowProgress && (
          isVideo ? (
            <>
              <div className="video-play-icon loading">
                <LoadingIndicator size="sm" />
              </div>
              <MediaProgressBadge label={progressLabel} value={progressBytesLabel || `${mediaProgress}%`} video />
            </>
          ) : (
            <MediaProgressBadge label={progressLabel} value={progressBytesLabel || `${mediaProgress}%`} />
          )
        )}

        {inlineLoading && !shouldShowProgress && (
          <div className="video-play-icon loading">
            <LoadingIndicator size="sm" />
          </div>
        )}

        {onCancelControl(messageId)}

        {inlineError && !inlineLoading && !shouldShowProgress && (
          <div
            className="media-status-chip error media-retry-chip"
            role="button"
            tabIndex={0}
            onClick={event => {
              event.stopPropagation();
              onInlinePlay(event);
            }}
            onKeyDown={event => {
              if (event.key === 'Enter' || event.key === ' ') {
                event.preventDefault();
                event.stopPropagation();
                onInlinePlay(event as unknown as React.MouseEvent);
              }
            }}
            title="Tentar reproduzir novamente"
          >
            <span>{inlineError}</span>
            <strong>Tentar novamente</strong>
          </div>
        )}

        {previewSrc && isVideo && !shouldShowProgress && !inlineLoading && (
          <div className="video-play-icon">
            <IconPlayerPlay size={24} stroke={2} color="white" />
          </div>
        )}

        {previewSrc && (shouldShowVideoSizeChip || (hasCachedFullMedia && !shouldShowProgress)) && (
          <div className="media-overlay-meta top-left">
            {shouldShowVideoSizeChip && (
              <span className="media-meta-label">
                {shouldShowProgress ? progressBytesLabel || mediaSizeLabel : mediaSizeLabel}
              </span>
            )}
            {hasCachedFullMedia && !shouldShowProgress && (
              <span className="media-cache-indicator" title="Mídia disponível no cache local" aria-label="Mídia em cache" />
            )}
          </div>
        )}

        {previewSrc && isVideo && videoDuration != null && (
          <div className="media-overlay-pill top-right">
            {formatDuration(videoDuration)}
          </div>
        )}

      </button>
    )}
  </div>
  );
};
