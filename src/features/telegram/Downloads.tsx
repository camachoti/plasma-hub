import React, { useEffect, useMemo, useState } from 'react';
import { IconAlertCircle, IconBrandInstagram, IconBrandReddit, IconBrandTelegram, IconBrandX, IconBrandYoutube, IconChevronDown, IconCircleCheck, IconCircleMinus, IconCloudDownload, IconDatabase, IconFileDownload, IconFolderOpen, IconLink, IconMessageCircle, IconRefresh, IconTrash, IconUsers, IconX } from "../../design-system/icons";
import { LoadingIndicator } from "../../design-system";
import '../../styles/Downloads.css';
import { downloadService, DownloadItem } from '../downloader/DownloadService';
import { analyzeUrl, downloadMedia } from '../downloader/downloader';
import type { MediaInfo } from '../downloader/types';
import { getDownloadDir, joinPath, openSystemPath, revealSystemItem } from '../../shared/platform/files';
import { runtimeCapabilities } from '../../shared/platform/runtime';
import { SHARED_DOWNLOAD_URL_EVENT, takePendingSharedDownloadUrl } from '../../shared/platform/sharedDownloadIntent';
import { debugWarn } from '../../shared/debug/logger';
import { Select } from '../../design-system';
import { EmptyState } from '../../design-system/EmptyState';
import { TextField } from '../../design-system';
import { DownloadArtwork } from '../downloader/DownloadArtwork';

function canDownloadFormat(media: MediaInfo, format?: MediaInfo['formats']['video'][number]) {
  if (!format || format.id === 'na' || format.id === 'web-limit') return false;
  if (media.platform === 'youtube' && runtimeCapabilities.isAndroid && format.hasAudio === false) return false;
  if (format.url) return true;
  if (
    media.platform === 'tiktok' &&
    Boolean(media.originalUrl) &&
    runtimeCapabilities.isTauri &&
    !runtimeCapabilities.isAndroid &&
    (format.id === 'best' || format.id === 'bestaudio')
  ) return true;
  return media.platform === 'youtube' && Boolean(media.originalUrl) && runtimeCapabilities.supportsNativeYoutube;
}

export const Downloads: React.FC = () => {
  const [downloads, setDownloads] = useState<DownloadItem[]>([]);
  const [downloadFilter, setDownloadFilter] = useState<'all' | 'active' | 'completed' | 'issues'>('all');
  const [expandedGroups, setExpandedGroups] = useState<Set<string>>(new Set());
  const [url, setUrl] = useState('');
  const [analyzing, setAnalyzing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [media, setMedia] = useState<MediaInfo | null>(null);
  const [selectedFormat, setSelectedFormat] = useState<string>('');

  useEffect(() => {
    setDownloads(Array.from(downloadService.activeDownloads.values()).reverse());
    const unsubscribe = downloadService.onDownloadsChange((list: DownloadItem[]) => {
      setDownloads([...list].reverse());
    });
    return () => unsubscribe();
  }, []);

  useEffect(() => {
    const applySharedUrl = (sharedUrl?: string) => {
      if (!sharedUrl) return;
      setUrl(sharedUrl);
      setError(null);
      setMedia(null);
      window.setTimeout(() => {
        setAnalyzing(true);
        analyzeUrl(sharedUrl)
          .then((info) => {
            setMedia(info);
            const formats = [...info.formats.video, ...info.formats.audio];
            const firstDownloadable = formats.find(f => canDownloadFormat(info, f));
            setSelectedFormat(firstDownloadable?.id || formats[0]?.id || '');
          })
          .catch((err: any) => setError(err.message || 'Erro ao analisar URL compartilhada'))
          .finally(() => setAnalyzing(false));
      }, 0);
    };

    applySharedUrl(takePendingSharedDownloadUrl() || undefined);

    const onSharedUrl = (event: Event) => {
      const sharedUrl = (event as CustomEvent<{ url?: string }>).detail?.url || takePendingSharedDownloadUrl();
      if (sharedUrl) takePendingSharedDownloadUrl();
      applySharedUrl(sharedUrl || undefined);
    };

    window.addEventListener(SHARED_DOWNLOAD_URL_EVENT, onSharedUrl);
    return () => window.removeEventListener(SHARED_DOWNLOAD_URL_EVENT, onSharedUrl);
  }, []);

  useEffect(() => {
    if (media) {
      const formats = [...media.formats.video, ...media.formats.audio];
      const currentValid = formats.some(f => f.id === selectedFormat && canDownloadFormat(media, f));
      if (!currentValid) {
        const nextFormat = formats.find(f => canDownloadFormat(media, f));
        setSelectedFormat(nextFormat?.id || formats[0]?.id || '');
      }
    }
  }, [media, selectedFormat]);

  const handleAnalyze = async () => {
    if (!url.trim()) return;
    setAnalyzing(true);
    setError(null);
    setMedia(null);
    try {
      const info = await analyzeUrl(url.trim());
      setMedia(info);
      const formats = [...info.formats.video, ...info.formats.audio];
      const firstDownloadable = formats.find(f => canDownloadFormat(info, f));
      setSelectedFormat(firstDownloadable?.id || formats[0]?.id || '');
    } catch (err: any) {
      setError(err.message || 'Erro ao analisar URL');
    } finally {
      setAnalyzing(false);
    }
  };

  const handleDownload = () => {
    if (!media || !selectedFormat) return;
    const selected = [...media.formats.video, ...media.formats.audio].find(f => f.id === selectedFormat);
    if (!selected || !canDownloadFormat(media, selected)) {
      setError(runtimeCapabilities.isAndroid && selected?.hasAudio === false
        ? 'Este formato separa video e audio. Escolha um formato com audio no Android.'
        : 'Este formato precisa do backend nativo do app.');
      return;
    }

    const mode = media.formats.video.some(f => f.id === selectedFormat) ? 'video' : 'audio';
    downloadMedia(media, mode, selectedFormat);
    setUrl('');
    setMedia(null);
  };

  const selectedDownloadable = media
    ? canDownloadFormat(media, [...media.formats.video, ...media.formats.audio].find(f => f.id === selectedFormat))
    : false;

  const handleOpenFolder = async (item?: DownloadItem) => {
    try {
      if (item?.filePath) {
        const parentDir = item.filePath.split(/[\\/]/).slice(0, -1).join('/') || item.filePath;
        await revealSystemItem(item.filePath).catch(() => openSystemPath(parentDir));
        return;
      }

      const dir = await getDownloadDir();
      if (item) {
        const fullPath = await joinPath(dir, item.fileName);
        await revealSystemItem(fullPath).catch(() => openSystemPath(dir));
      } else {
        await openSystemPath(dir);
      }
    } catch (e) {
      debugWarn('Failed to open folder:', e);
    }
  };

  const platformLabel = (platform?: DownloadItem['platform']) => {
    if (!platform) return null;
    const labels: Record<string, string> = {
      telegram: 'Telegram',
      youtube: 'YouTube',
      tiktok: 'TikTok',
      instagram: 'Instagram',
      twitter: 'Twitter/X',
      reddit: 'Reddit',
      web: 'Web',
    };
    return labels[platform] || platform;
  };

  const formatBytes = (bytes?: number) => {
    if (!bytes || bytes <= 0) return null;
    const units = ['B', 'KB', 'MB', 'GB', 'TB'];
    const exponent = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
    const value = bytes / Math.pow(1024, exponent);
    return `${Number(value.toFixed(value >= 10 || exponent === 0 ? 0 : 1))} ${units[exponent]}`;
  };

  const filteredDownloads = useMemo(() => downloads.filter(item => {
    if (downloadFilter === 'active') return item.status === 'downloading';
    if (downloadFilter === 'completed') return item.status === 'completed';
    if (downloadFilter === 'issues') return item.status === 'failed' || item.status === 'canceled';
    return true;
  }), [downloadFilter, downloads]);

  const groupedDownloads = useMemo(() => {
    const groups = new Map<string, DownloadItem[]>();
    const entries: Array<{ type: 'single'; item: DownloadItem } | { type: 'group'; id: string; items: DownloadItem[] }> = [];

    for (const item of filteredDownloads) {
      if (item.batchId) {
        if (!groups.has(item.batchId)) groups.set(item.batchId, []);
        groups.get(item.batchId)!.push(item);
      } else {
        entries.push({ type: 'single', item });
      }
    }

    for (const [id, items] of groups) {
      entries.push({ type: 'group', id, items });
    }

    return entries.sort((a, b) => {
      const aItem = a.type === 'single' ? a.item : a.items[0];
      const bItem = b.type === 'single' ? b.item : b.items[0];
      return filteredDownloads.indexOf(aItem) - filteredDownloads.indexOf(bItem);
    });
  }, [filteredDownloads]);

  const groupSummary = (items: DownloadItem[]) => {
    const visibleTotal = items.length || 1;
    const reportedTotal = Math.max(0, ...items.map(item => item.batchTotal || 0));
    const total = Math.max(visibleTotal, reportedTotal || 0) || 1;
    const hasRetryHistory = items.some(item => item.retryAttempted);
    const visibleCompleted = items.filter(item => item.status === 'completed').length;
    const visibleFailed = items.filter(item => item.status === 'failed').length;
    const visibleCanceled = items.filter(item => item.status === 'canceled').length;
    const completed = hasRetryHistory ? visibleCompleted : Math.max(visibleCompleted, ...items.map(item => item.batchCompleted || 0));
    const skipped = Math.max(0, ...items.map(item => item.batchSkipped || 0));
    const failed = hasRetryHistory ? visibleFailed : Math.max(visibleFailed, ...items.map(item => item.batchFailed || 0));
    const canceled = visibleCanceled;
    const running = items.filter(item => item.status === 'downloading').length;
    const reportedDownloaded = Math.max(0, ...items.map(item => item.batchDownloaded || 0));
    const retryingItems = items.filter(item => item.retrying && item.status === 'downloading');
    const retryProgress = completed + skipped + retryingItems.reduce((sum, item) => sum + Math.max(0, Math.min(100, item.progress || 0)) / 100, 0);
    const progress = hasRetryHistory
      ? Math.round((Math.min(total, retryProgress) / total) * 100)
      : reportedTotal > 0
      ? Math.round((Math.min(total, reportedDownloaded) / total) * 100)
      : Math.round(items.reduce((sum, item) => {
        if (item.status === 'completed') return sum + 100;
        if (item.status === 'failed') return sum + 100;
        if (item.status === 'canceled') return sum + 100;
        return sum + Math.max(0, Math.min(100, item.progress || 0));
      }, 0) / visibleTotal);
    const hasPendingBatchItems = reportedTotal > 0 && reportedDownloaded < total;
    const status: DownloadItem['status'] = hasPendingBatchItems || running > 0
      ? 'downloading'
      : failed > 0
        ? 'failed'
        : canceled > 0
          ? 'canceled'
        : 'completed';
    return { total, completed, skipped, failed, canceled, running, status, progress };
  };

  const toggleGroup = (id: string) => {
    setExpandedGroups(current => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const runningCount = downloads.filter(d => d.status === 'downloading').length;
  const finishedCount = downloads.length - runningCount;
  const actionableRunningCount = downloads.filter(d => d.status === 'downloading' && d.canCancel).length;

  const handleCancelDownload = async (item: DownloadItem) => {
    await downloadService.cancelDownload(item.id);
  };

  const handleRetryDownload = async (item: DownloadItem) => {
    await downloadService.retryDownload(item.id);
  };

  const handleCancelGroup = async (items: DownloadItem[]) => {
    await Promise.all(items.filter(item => item.status === 'downloading' && item.canCancel).map(item => downloadService.cancelDownload(item.id)));
  };

  const handleRetryGroup = async (items: DownloadItem[]) => {
    await Promise.all(items.filter(item => (item.status === 'failed' || item.status === 'canceled') && item.canRetry).map(item => downloadService.retryDownload(item.id)));
  };

  const renderDownloadItem = (item: DownloadItem, compact = false) => (
    <div
      key={item.id}
      className={`download-item ${compact ? 'compact' : ''} ${item.status === 'completed' ? 'clickable' : ''}`}
      onClick={item.status === 'completed' ? () => handleOpenFolder(item) : undefined}
    >
      <DownloadArtwork item={item} />

      <div className="download-details">
        <div className="download-name-row">
          <h3 className="download-name" title={item.fileName}>{item.fileName}</h3>
          <span className={`download-percentage status-${item.status}`}>
            {item.status === 'downloading' ? `${Math.round(item.progress)}%` :
             item.status === 'completed' ? 'Concluído' :
             item.status === 'canceled' ? 'Cancelado' : 'Erro'}
          </span>
        </div>

        <div className={`progress-bar-container ${item.status}`}>
          <div
            className="progress-bar-fill"
            style={{ width: `${item.status === 'completed' ? 100 : item.progress}%` }}
          />
        </div>

        <div className="download-item-meta">
          {item.platform && <span className="download-item-meta-part">{item.platform === 'telegram' ? <IconBrandTelegram size={17} stroke={1.8} /> : item.platform === 'twitter' ? <IconBrandX size={17} stroke={1.8} /> : item.platform === 'youtube' ? <IconBrandYoutube size={17} stroke={1.8} /> : item.platform === 'instagram' ? <IconBrandInstagram size={17} stroke={1.8} /> : item.platform === 'reddit' ? <IconBrandReddit size={17} stroke={1.8} /> : <IconCloudDownload size={17} stroke={1.8} />}<span>{platformLabel(item.platform)}</span></span>}
          {formatBytes(item.fileSize) && <span className="download-item-meta-part"><IconDatabase size={17} stroke={1.8} /><span>{formatBytes(item.fileSize)}</span></span>}
          {compact && !formatBytes(item.fileSize) && <span className="download-item-meta-part is-unavailable" title="Este download não tem tamanho registrado no histórico"><IconDatabase size={17} stroke={1.8} /><span>Tamanho não registrado</span></span>}
          {item.chatTitle && <span className="download-item-meta-part"><IconUsers size={17} stroke={1.8} /><span>{item.chatKind === 'grupo' ? 'Grupo' : item.chatKind === 'canal' ? 'Canal' : item.chatKind === 'twitter' ? 'Twitter' : 'Chat'}: {item.chatTitle}</span></span>}
          {item.topicTitle && <span className="download-item-meta-part"><IconMessageCircle size={17} stroke={1.8} /><span>Tópico: {item.topicTitle}</span></span>}
          {item.sourceLabel && !item.chatTitle && <span className="download-item-meta-part"><IconLink size={17} stroke={1.8} /><span>Origem: {item.sourceLabel}</span></span>}
          {item.senderName && <span className="download-item-meta-part"><IconUsers size={17} stroke={1.8} /><span>Usuário: {item.senderName}</span></span>}
        </div>
        {item.error && <div className="download-item-error" title={item.error}>{item.error}</div>}
      </div>
      <div className="download-item-actions">
        {item.status === 'downloading' && item.canCancel && (
          <button
            type="button"
            className="download-action-icon danger"
            title="Cancelar download"
            aria-label={`Cancelar ${item.fileName}`}
            onClick={event => {
              event.stopPropagation();
              void handleCancelDownload(item);
            }}
          >
            <IconX size={16} stroke={2} />
          </button>
        )}
        {(item.status === 'failed' || item.status === 'canceled') && item.canRetry && (
          <button
            type="button"
            className="download-action-button"
            title={item.error?.includes('interrompido') ? 'Retomar download interrompido' : 'Tentar novamente'}
            aria-label={`${item.error?.includes('interrompido') ? 'Retomar' : 'Tentar novamente'} ${item.fileName}`}
            onClick={event => {
              event.stopPropagation();
              void handleRetryDownload(item);
            }}
          >
            <IconRefresh size={15} stroke={2} />
            {item.error?.includes('interrompido') ? 'Retomar' : 'Tentar novamente'}
          </button>
        )}
        {item.status !== 'downloading' && (
          <button
            type="button"
            className="download-action-icon danger"
            title="Remover do histórico"
            aria-label={`Remover ${item.fileName} do histórico`}
            onClick={event => {
              event.stopPropagation();
              downloadService.removeDownload(item.id);
            }}
          >
            <IconTrash size={16} stroke={2} />
          </button>
        )}
      </div>
    </div>
  );

  return (
    <div className="downloads-container">
      <header className="downloads-topbar">
        <div className="downloads-title-block">
          <h1>Downloads</h1>
          <span>{downloads.length} itens</span>
        </div>
        <div className="downloads-topbar-actions">
          {finishedCount > 0 && (
            <button className="text-action-btn" onClick={() => downloadService.clearFinished()}>
              <IconTrash size={15} stroke={2} /> Limpar finalizados
            </button>
          )}
          {actionableRunningCount > 0 && (
            <button
              className="text-action-btn danger"
              onClick={() => {
                void Promise.all(downloads
                  .filter(item => item.status === 'downloading' && item.canCancel)
                  .map(item => downloadService.cancelDownload(item.id)));
              }}
            >
              <IconX size={15} stroke={2} /> Cancelar ativos
            </button>
          )}
          <button className="folder-action-btn" onClick={() => handleOpenFolder()} title="Abrir pasta de downloads">
            <IconFolderOpen size={16} stroke={2} /> Pasta
          </button>
        </div>
      </header>

      <div className="downloads-content">
        <section className="quick-import-card">
          <div className="quick-import-header">
            <div className="quick-import-title">
              <IconLink size={16} stroke={2} /> Importar link
            </div>

            <div className="supported-tags">
              <span className="supported-label">COMPATÍVEL COM:</span>
              <span className="supported-icon" title="YouTube" aria-label="YouTube"><IconBrandYoutube size={18} stroke={2} /></span>
              <span className="supported-icon" title="Reddit" aria-label="Reddit"><IconBrandReddit size={18} stroke={2} /></span>
              <span className="supported-icon" title="Twitter/X" aria-label="Twitter/X"><IconBrandX size={18} stroke={2} /></span>
              <span className="supported-icon" title="Instagram" aria-label="Instagram"><IconBrandInstagram size={18} stroke={2} /></span>
            </div>
          </div>

          <div className="downloader-input-group">
            <TextField
              type="text"
              placeholder="Cole aqui um link do YouTube, Reddit, X/Twitter ou Instagram..."
              aria-label="Link da mídia para analisar"
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && handleAnalyze()}
            />
            <button
              className="analyze-btn"
              onClick={handleAnalyze}
              disabled={analyzing || !url.trim()}
              aria-busy={analyzing}
            >
                {analyzing ? <><LoadingIndicator size="sm" /> Analisando…</> : (
                <>Analisar <IconCloudDownload size={18} stroke={2} /></>
              )}
            </button>
          </div>

        {error && <div className="error-message" role="alert">{error}</div>}

        {media && (
          <div className="media-preview-card">
            {media.thumbnailUrl && (
              <img src={media.thumbnailUrl} alt="Thumbnail" className="media-thumbnail" />
            )}
            <div className="media-info">
              <h3>{media.title}</h3>
              <p className="media-author">{media.author} • {media.duration}</p>
              
              <div className="format-selection">
                <Select value={selectedFormat} onChange={setSelectedFormat} ariaLabel="Formato para download" options={[
                  ...media.formats.video.map(f => ({ value: f.id, label: `Vídeo · ${f.label} ${f.size !== '—' ? `(${f.size})` : ''}`, disabled: !canDownloadFormat(media, f) })),
                  ...media.formats.audio.map(f => ({ value: f.id, label: `Áudio · ${f.label} ${f.size !== '—' ? `(${f.size})` : ''}`, disabled: !canDownloadFormat(media, f) })),
                ]} />
                <button className="download-btn" onClick={handleDownload} disabled={!selectedDownloadable}>
                  <IconCloudDownload size={18} stroke={2} /> Confirmar
                </button>
              </div>
            </div>
          </div>
        )}
        </section>

        <div className="downloads-section-header">
          <h2 className="downloads-section-title">
            Atividade
            {runningCount > 0 && (
              <span className="running-badge">{runningCount} EM ANDAMENTO</span>
            )}
          </h2>
          <div className="download-filter-tabs" role="tablist" aria-label="Filtrar downloads">
            {[
              ['all', 'Todos'],
              ['active', 'Ativos'],
              ['completed', 'Concluídos'],
              ['issues', 'Falhas'],
            ].map(([value, label]) => (
              <button
                key={value}
                type="button"
                role="tab"
                aria-selected={downloadFilter === value}
                className={downloadFilter === value ? 'active' : ''}
                onClick={() => setDownloadFilter(value as typeof downloadFilter)}
              >
                {label}
              </button>
            ))}
          </div>
        </div>

        {filteredDownloads.length === 0 ? (
          <EmptyState
            className="downloads-empty"
            icon={<IconCloudDownload size={32} stroke={1.5} />}
            title={downloads.length ? 'Nenhum item neste filtro' : 'Seus downloads começam aqui'}
            description={downloads.length ? 'Escolha outro filtro para consultar os demais downloads.' : 'Cole um link acima para analisar e iniciar seu primeiro download.'}
          />
        ) : (
          <div className="downloads-list">
            {groupedDownloads.map(entry => {
              if (entry.type === 'single') return renderDownloadItem(entry.item);

              const summary = groupSummary(entry.items);
              const first = entry.items[0];
              const expanded = expandedGroups.has(entry.id);
              const groupSize = formatBytes(entry.items.reduce((sum, item) => sum + (item.fileSize || 0), 0));
              const groupPlatform = platformLabel(first.platform);
              const chatKind = first.chatKind === 'grupo' ? 'Grupo' : first.chatKind === 'canal' ? 'Canal' : first.chatKind === 'twitter' ? 'Twitter' : 'Chat';
              const groupCanCancel = entry.items.some(item => item.status === 'downloading' && item.canCancel);
              const groupCanRetry = entry.items.some(item => (item.status === 'failed' || item.status === 'canceled') && item.canRetry);
              const groupHasRetryableIssue = summary.failed > 0 || summary.canceled > 0;
              const artworkItem = entry.items.find(item => item.thumbnailUrl || (item.status === 'completed' && /\.(?:avif|bmp|gif|jpe?g|png|webp|avi|m4v|mkv|mov|mp4|mpeg|mpg)$/i.test(item.filePath || ''))) || first;

              return (
                <div key={entry.id} className={`download-group ${summary.status}`}>
                  <div className="download-group-overview">
                    <DownloadArtwork key={artworkItem.id} item={artworkItem} status={summary.status} group />
                    <div className="download-group-body">
                      <button
                        type="button"
                        className="download-group-title-button"
                        aria-expanded={expanded}
                        onClick={() => toggleGroup(entry.id)}
                      >
                        <span className="download-name" title={first.batchTitle || first.fileName}>{first.batchTitle || 'Download em lote'}</span>
                        <span className={`download-percentage status-${summary.status}`}>
                          {summary.status === 'failed' ? 'Erro' : summary.status === 'canceled' ? 'Cancelado' : `${summary.progress}%`}
                        </span>
                        <IconChevronDown className={`download-group-caret ${expanded ? 'open' : ''}`} size={18} stroke={2} />
                      </button>

                      <div className={`progress-bar-container ${summary.status}`}>
                        <div className="progress-bar-fill" style={{ width: `${summary.progress}%` }} />
                      </div>

                      <div className="download-summary-metrics">
                        <span className="download-summary-metric"><IconFileDownload size={20} stroke={1.8} /><strong>{summary.total}</strong><span>arquivos</span></span>
                        <span className="download-summary-metric is-complete"><IconCircleCheck size={21} stroke={1.9} /><strong>{summary.completed}</strong><span>finalizados</span></span>
                        <span className="download-summary-metric"><IconCircleMinus size={21} stroke={1.9} /><strong>{summary.skipped}</strong><span>ignorados</span></span>
                        <span className={`download-summary-metric ${summary.failed ? 'has-error' : ''}`}><IconAlertCircle size={21} stroke={1.9} /><strong>{summary.failed}</strong><span>falharam</span></span>
                        {summary.running > 0 && <span className="download-summary-metric"><IconCloudDownload size={20} stroke={1.8} /><strong>{summary.running}</strong><span>em andamento</span></span>}
                        {summary.canceled > 0 && <span className="download-summary-metric is-muted"><IconCircleMinus size={21} stroke={1.9} /><strong>{summary.canceled}</strong><span>cancelados</span></span>}
                        {groupPlatform && <span className="download-summary-metric is-source">{first.platform === 'telegram' ? <IconBrandTelegram size={20} stroke={1.8} /> : first.platform === 'twitter' ? <IconBrandX size={20} stroke={1.8} /> : first.platform === 'youtube' ? <IconBrandYoutube size={20} stroke={1.8} /> : first.platform === 'instagram' ? <IconBrandInstagram size={20} stroke={1.8} /> : first.platform === 'reddit' ? <IconBrandReddit size={20} stroke={1.8} /> : <IconCloudDownload size={20} stroke={1.8} />}<span>{groupPlatform}</span></span>}
                        {groupSize && <span className="download-summary-metric is-source"><IconDatabase size={20} stroke={1.8} /><span>{groupSize}</span></span>}
                      </div>
                    </div>
                  </div>

                  <div className="download-group-bottom-row">
                    <div className="download-group-context">
                      {(first.chatTitle || first.sourceLabel) && <span className="download-context-chip" title={first.chatTitle || first.sourceLabel}><IconUsers size={19} stroke={1.8} /><span>{first.chatTitle ? `${chatKind}:` : 'Origem:'}</span><strong>{first.chatTitle || first.sourceLabel}</strong></span>}
                      {first.topicTitle && <span className="download-context-chip" title={first.topicTitle}><IconMessageCircle size={19} stroke={1.8} /><span>Tópico:</span><strong>{first.topicTitle}</strong></span>}
                    </div>
                    {(groupCanCancel || groupHasRetryableIssue) && (
                      <div className="download-group-actions">
                        {groupCanCancel && <button type="button" className="download-action-icon danger" title="Cancelar itens ativos" aria-label="Cancelar itens ativos" onClick={() => void handleCancelGroup(entry.items)}><IconX size={16} stroke={2} /></button>}
                        {groupHasRetryableIssue && (
                          <button type="button" className="download-action-button download-group-retry" title={groupCanRetry ? 'Retomar itens com falha' : 'Não há itens recuperáveis neste lote'} aria-label="Retomar itens com falha" disabled={!groupCanRetry} onClick={() => void handleRetryGroup(entry.items)}>
                            <IconRefresh size={17} stroke={2} />
                            {summary.failed > 0 ? `Retomar ${summary.failed} ${summary.failed === 1 ? 'falha' : 'falhas'}` : 'Retomar itens'}
                          </button>
                        )}
                      </div>
                    )}
                  </div>

                  {expanded && (
                    <div className="download-group-items">
                      {entry.items.map(item => renderDownloadItem(item, true))}
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
};
