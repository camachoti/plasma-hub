import React from 'react';
import { Play, SpeakerHigh, SpeakerSlash } from '@phosphor-icons/react';
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
}) => (
  <div className={`media-preview ${isVideo ? 'is-video' : 'is-image'} ${shouldRenderInlinePlayer ? 'playing-inline' : ''}`}>
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
          onLoadedMetadata={event => onLogVideoEvent('inline loadedmetadata', event.currentTarget, { chatId, messageId, src: inlineStreamUrl })}
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
          className="inline-mute-btn"
          onClick={(event) => {
            event.stopPropagation();
            onSetMuted(!isMuted);
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
          <MediaProgressBadge label={progressLabel} value={progressBytesLabel || `${mediaProgress}%`} video />
        )}
        {onCancelControl(messageId)}
      </div>
    ) : (
      <button
        type="button"
        className="media-preview-button"
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
          />
        ) : isVideo ? (
          <div className="video-preview-unavailable" onContextMenu={onContextMenu}>
            <span className="video-preview-unavailable-icon"><Play size={20} weight="fill" /></span>
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
                <span className="spinner"></span>
              </div>
              <MediaProgressBadge label={progressLabel} value={progressBytesLabel || `${mediaProgress}%`} video />
            </>
          ) : (
            <MediaProgressBadge label={progressLabel} value={progressBytesLabel || `${mediaProgress}%`} />
          )
        )}

        {inlineLoading && !shouldShowProgress && (
          <div className="video-play-icon loading">
            <span className="spinner"></span>
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
            <Play size={24} weight="fill" color="white" />
          </div>
        )}

        {previewSrc && (shouldShowVideoSizeChip || (hasCachedFullMedia && !shouldShowProgress)) && (
          <div className="media-overlay-meta top-left">
            {shouldShowVideoSizeChip && <span className="media-meta-label">{mediaSizeLabel}</span>}
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
