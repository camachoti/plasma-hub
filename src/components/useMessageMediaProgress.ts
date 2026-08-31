import { formatMediaBytes, normalizeMediaBytes } from './MessageMediaPrimitives';

interface AlbumMediaSize {
  id: number;
  mediaSize?: number | null;
}

interface UseMessageMediaProgressOptions {
  activeMessageId: number;
  albumMedias?: AlbumMediaSize[];
  mediaSize?: number | null;
  mediaBytes: {
    downloadedBytes?: number;
    totalBytes?: number;
  };
  mediaProgress: number;
  mediaStage: string | null;
  playerProgress: number;
  loadingFullMedia: boolean;
  savingMedia: boolean;
  cancelingMedia: boolean;
  isVideo: boolean;
  isInlinePlaying: boolean;
  hasCachedFullMedia: boolean;
}

export function useMessageMediaProgress({
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
}: UseMessageMediaProgressOptions) {
  const isSavingInBackground = savingMedia || (mediaStage === 'saving' || mediaStage === 'downloading') && mediaProgress > 0 && mediaProgress < 100;
  const shouldShowProgress = loadingFullMedia || isSavingInBackground;
  const visiblePlayerProgress = playerProgress > 0 && playerProgress < 100 ? playerProgress : mediaProgress;
  const activeMediaSize = normalizeMediaBytes(albumMedias?.find(item => item.id === activeMessageId)?.mediaSize ?? mediaSize);
  const knownTotalBytes = normalizeMediaBytes(mediaBytes.totalBytes) || activeMediaSize;
  const knownDownloadedBytes = mediaBytes.downloadedBytes
    ?? (knownTotalBytes && visiblePlayerProgress > 0 ? Math.round((visiblePlayerProgress / 100) * knownTotalBytes) : undefined);
  const progressBytesLabel = knownTotalBytes
    ? `${formatMediaBytes(knownDownloadedBytes) || '0 B'} / ${formatMediaBytes(knownTotalBytes)}`
    : formatMediaBytes(knownDownloadedBytes);
  const progressDetailLabel = progressBytesLabel || (visiblePlayerProgress > 0 && visiblePlayerProgress < 100 ? `${visiblePlayerProgress}%` : null);
  const progressLabel = savingMedia || mediaStage === 'saving'
    ? 'Salvando'
    : mediaStage === 'downloading'
      ? 'Baixando'
      : mediaStage === 'processing'
        ? 'Processando'
        : 'Preparando';
  const mediaSizeLabel = formatMediaBytes(normalizeMediaBytes(mediaSize));
  const shouldShowVideoSizeChip = Boolean(isVideo && mediaSizeLabel && !isInlinePlaying && !hasCachedFullMedia);
  const hasCancelableMediaProgress = (
    mediaStage === 'downloading'
    || mediaStage === 'saving'
  ) && visiblePlayerProgress > 0 && visiblePlayerProgress < 100;
  const canCancelMediaDownload = !cancelingMedia && (savingMedia || isSavingInBackground || hasCancelableMediaProgress);

  return {
    canCancelMediaDownload,
    mediaSizeLabel,
    progressBytesLabel,
    progressDetailLabel,
    progressLabel,
    shouldShowProgress,
    shouldShowVideoSizeChip,
    visiblePlayerProgress,
  };
}
