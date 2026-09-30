import React from 'react';
import { createPortal } from 'react-dom';
import { IconArrowsMaximize, IconArrowsMinimize, IconDownload, IconX } from "../design-system/icons";
import { ContextMenu } from './ContextMenu';
import { MessageMediaMini } from './MessageMediaPrimitives';

type ContextMenuItems = React.ComponentProps<typeof ContextMenu>['items'];

interface AlbumMedia {
  id: number;
  isVideo: boolean;
  videoDuration?: number | null;
  messageDate?: number;
  mediaSize?: number | null;
}

interface ContextMenuState {
  visible: boolean;
  x: number;
  y: number;
}

interface MessageMediaLightboxProps {
  palette?: string;
  density?: string;
  chatId: string;
  activeMessageId: number;
  activeIsVideo: boolean;
  activeFullSrc: string | null;
  activeError: string | null;
  activeLoading: boolean;
  previewSrc: string | null;
  savingMedia: boolean;
  cancelingMedia: boolean;
  lightboxBuffering: boolean;
  shouldShowProgress: boolean;
  progressLabel: string;
  progressBytesLabel: string | null;
  mediaProgress: number;
  visiblePlayerProgress: number;
  progressDetailLabel: string | null;
  isPlayerFullscreen: boolean;
  albumMedias?: AlbumMedia[];
  contextMenu: ContextMenuState;
  contextMenuItems: ContextMenuItems;
  videoShellRef: React.RefObject<HTMLDivElement | null>;
  onClose: () => void;
  onSave: (event: React.MouseEvent) => void;
  onToggleFullscreen: (event: React.MouseEvent) => void;
  onContextMenu: (event: React.MouseEvent) => void;
  onCancelControl: (targetMessageId?: number) => React.ReactNode;
  onSelectAlbumMedia: (messageId: number) => void;
  onSetLightboxBuffering: (buffering: boolean) => void;
  onSetActiveError: (error: string | null) => void;
  onSetActiveFullSrc: (src: string | null) => void;
  onScheduleFullMediaPrefetch: (messageId: number) => void;
  onCloseContextMenu: () => void;
  onLogVideoEvent: (label: string, video: HTMLVideoElement | null, context: Record<string, any>) => void;
}

export const MessageMediaLightbox: React.FC<MessageMediaLightboxProps> = ({
  palette,
  density,
  chatId,
  activeMessageId,
  activeIsVideo,
  activeFullSrc,
  activeError,
  activeLoading,
  previewSrc,
  savingMedia,
  cancelingMedia,
  lightboxBuffering,
  shouldShowProgress,
  progressLabel,
  progressBytesLabel,
  mediaProgress,
  visiblePlayerProgress,
  progressDetailLabel,
  isPlayerFullscreen,
  albumMedias,
  contextMenu,
  contextMenuItems,
  videoShellRef,
  onClose,
  onSave,
  onToggleFullscreen,
  onContextMenu,
  onCancelControl,
  onSelectAlbumMedia,
  onSetLightboxBuffering,
  onSetActiveError,
  onSetActiveFullSrc,
  onScheduleFullMediaPrefetch,
  onCloseContextMenu,
  onLogVideoEvent,
}) => createPortal(
  <div
    className="media-lightbox"
    data-palette={palette}
    data-density={density}
    onClick={onClose}
  >
    <div className="media-lightbox-toolbar" onClick={event => event.stopPropagation()}>
      <button
        type="button"
        className="btn-icon"
        onClick={onSave}
        disabled={cancelingMedia}
        title={savingMedia ? 'Cancelar download' : 'Download'}
        aria-label={savingMedia ? 'Cancelar download da mídia' : 'Salvar mídia'}
      >
        {savingMedia ? <IconX size={20} stroke={2} /> : <IconDownload size={20} stroke={2} />}
      </button>
      <button
        type="button"
        className="btn-icon"
        onClick={onClose}
        aria-label="Fechar visualizacao de midia"
      >
        ×
      </button>
    </div>
    <div className={`media-lightbox-content ${activeIsVideo ? 'video-content' : 'image-content'}`}>
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
            {onCancelControl(activeMessageId)}
          </div>
        </div>
      ) : activeIsVideo && activeFullSrc && !activeError ? (
        <div className="media-video-shell" ref={videoShellRef}>
          <video
            className="media-video-player"
            src={activeFullSrc}
            poster={previewSrc || undefined}
            controls
            controlsList="nofullscreen"
            autoPlay
            playsInline
            preload="auto"
            onLoadStart={event => onLogVideoEvent('lightbox loadstart', event.currentTarget, { chatId, messageId: activeMessageId, src: activeFullSrc })}
            onLoadedMetadata={event => {
              onSetLightboxBuffering(false);
              onLogVideoEvent('lightbox loadedmetadata', event.currentTarget, { chatId, messageId: activeMessageId, src: activeFullSrc });
            }}
            onLoadedData={event => onLogVideoEvent('lightbox loadeddata', event.currentTarget, { chatId, messageId: activeMessageId, src: activeFullSrc })}
            onProgress={event => onLogVideoEvent('lightbox progress', event.currentTarget, { chatId, messageId: activeMessageId, src: activeFullSrc })}
            onSuspend={event => onLogVideoEvent('lightbox suspend', event.currentTarget, { chatId, messageId: activeMessageId, src: activeFullSrc })}
            onAbort={event => onLogVideoEvent('lightbox abort', event.currentTarget, { chatId, messageId: activeMessageId, src: activeFullSrc })}
            onCanPlay={event => {
              onSetLightboxBuffering(false);
              onLogVideoEvent('lightbox canplay', event.currentTarget, { chatId, messageId: activeMessageId, src: activeFullSrc });
            }}
            onPlaying={event => {
              onSetLightboxBuffering(false);
              onScheduleFullMediaPrefetch(activeMessageId);
              onLogVideoEvent('lightbox playing', event.currentTarget, { chatId, messageId: activeMessageId, src: activeFullSrc });
            }}
            onWaiting={event => {
              onSetLightboxBuffering(true);
              onLogVideoEvent('lightbox waiting', event.currentTarget, { chatId, messageId: activeMessageId, src: activeFullSrc });
            }}
            onSeeking={event => {
              onSetLightboxBuffering(true);
              onLogVideoEvent('lightbox seeking', event.currentTarget, { chatId, messageId: activeMessageId, src: activeFullSrc });
            }}
            onSeeked={() => onSetLightboxBuffering(false)}
            onStalled={event => {
              onSetLightboxBuffering(true);
              onLogVideoEvent('lightbox stalled', event.currentTarget, { chatId, messageId: activeMessageId, src: activeFullSrc });
            }}
            onError={event => {
              onLogVideoEvent('lightbox error', event.currentTarget, { chatId, messageId: activeMessageId, src: activeFullSrc });
              onSetLightboxBuffering(false);
              onSetActiveError('Falha ao reproduzir vídeo.');
              onSetActiveFullSrc(null);
            }}
            onContextMenu={onContextMenu}
            onClick={event => event.stopPropagation()}
          />
          <button
            type="button"
            className="media-player-fullscreen-btn"
            onClick={onToggleFullscreen}
            aria-label={isPlayerFullscreen ? 'Sair da tela cheia' : 'Entrar em tela cheia'}
            title={isPlayerFullscreen ? 'Sair da tela cheia' : 'Tela cheia'}
          >
            {isPlayerFullscreen ? <IconArrowsMinimize size={20} stroke={2} /> : <IconArrowsMaximize size={20} stroke={2} />}
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
          onContextMenu={onContextMenu}
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
      <div className="media-lightbox-carousel" onClick={event => event.stopPropagation()}>
        {albumMedias.map(item => (
          <button
            key={item.id}
            className={`carousel-thumb ${item.id === activeMessageId ? 'active' : ''}`}
            onClick={() => onSelectAlbumMedia(item.id)}
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
        onClose={onCloseContextMenu}
        palette={palette}
        density={density}
      />
    )}
  </div>,
  document.querySelector('.dashboard-container') || document.body,
);
