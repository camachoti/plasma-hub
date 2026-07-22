import React, { useEffect, useMemo, useState } from 'react';
import { CloudArrowDown, CheckCircle, WarningCircle, FileArrowDown, HardDrive, Link, Spinner, FolderOpen, YoutubeLogo, RedditLogo, TwitterLogo, InstagramLogo, StackSimple, CaretDown, XCircle } from '@phosphor-icons/react';
import '../../styles/Downloads.css';
import { downloadService, DownloadItem } from '../downloader/DownloadService';
import { analyzeUrl, downloadMedia } from '../downloader/downloader';
import type { MediaInfo } from '../downloader/types';
import { getDownloadDir, joinPath, openSystemPath, revealSystemItem } from '../../shared/platform/files';
import { runtimeCapabilities } from '../../shared/platform/runtime';
import { SHARED_DOWNLOAD_URL_EVENT, takePendingSharedDownloadUrl } from '../../shared/platform/sharedDownloadIntent';
import { debugWarn } from '../../shared/debug/logger';

function canDownloadFormat(media: MediaInfo, format?: MediaInfo['formats']['video'][number]) {
  if (!format || format.id === 'na' || format.id === 'web-limit') return false;
  if (media.platform === 'youtube' && runtimeCapabilities.isAndroid && format.hasAudio === false) return false;
  if (format.url) return true;
  return media.platform === 'youtube' && Boolean(media.originalUrl) && runtimeCapabilities.supportsNativeYoutube;
}

export const Downloads: React.FC = () => {
  const [downloads, setDownloads] = useState<DownloadItem[]>([]);
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

  const downloadMetaItems = (item: DownloadItem) => [
    platformLabel(item.platform),
    formatBytes(item.fileSize),
    item.sourceLabel ? `Origem: ${item.sourceLabel}` : null,
    item.chatTitle ? `${item.chatKind === 'grupo' ? 'Grupo' : item.chatKind === 'canal' ? 'Canal' : item.chatKind === 'twitter' ? 'Twitter' : 'Chat'}: ${item.chatTitle}` : null,
    item.topicTitle ? `Tópico: ${item.topicTitle}` : null,
    item.senderName ? `Usuário: ${item.senderName}` : null,
  ].filter(Boolean);

  const groupedDownloads = useMemo(() => {
    const groups = new Map<string, DownloadItem[]>();
    const entries: Array<{ type: 'single'; item: DownloadItem } | { type: 'group'; id: string; items: DownloadItem[] }> = [];

    for (const item of downloads) {
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
      return downloads.indexOf(aItem) - downloads.indexOf(bItem);
    });
  }, [downloads]);

  const groupSummary = (items: DownloadItem[]) => {
    const visibleTotal = items.length || 1;
    const reportedTotal = Math.max(0, ...items.map(item => item.batchTotal || 0));
    const total = Math.max(visibleTotal, reportedTotal || 0) || 1;
    const visibleCompleted = items.filter(item => item.status === 'completed').length;
    const visibleFailed = items.filter(item => item.status === 'failed').length;
    const visibleCanceled = items.filter(item => item.status === 'canceled').length;
    const completed = Math.max(visibleCompleted, ...items.map(item => item.batchCompleted || 0));
    const skipped = Math.max(0, ...items.map(item => item.batchSkipped || 0));
    const failed = Math.max(visibleFailed, ...items.map(item => item.batchFailed || 0));
    const canceled = visibleCanceled;
    const running = items.filter(item => item.status === 'downloading').length;
    const reportedDownloaded = Math.max(0, ...items.map(item => item.batchDownloaded || 0));
    const progress = reportedTotal > 0
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
    const thumbnail = items.find(item => item.thumbnailUrl)?.thumbnailUrl;
    return { total, completed, skipped, failed, canceled, running, status, progress, thumbnail };
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

  const renderDownloadItem = (item: DownloadItem, compact = false) => (
    <div
      key={item.id}
      className={`download-item ${compact ? 'compact' : ''} ${item.status === 'completed' ? 'clickable' : ''}`}
      onClick={item.status === 'completed' ? () => handleOpenFolder(item) : undefined}
    >
      <div className="download-icon-wrapper">
        {item.thumbnailUrl ? (
          <div className="download-thumbnail-container">
            <img src={item.thumbnailUrl} alt="Thumbnail" className="download-thumbnail-img" />
            <div className={`download-status-overlay ${item.status}`}>
              {item.status === 'downloading' && <FileArrowDown size={14} />}
              {item.status === 'completed' && <CheckCircle size={14} />}
              {item.status === 'failed' && <WarningCircle size={14} />}
              {item.status === 'canceled' && <XCircle size={14} />}
            </div>
          </div>
        ) : (
          <div className={`download-icon ${item.status}`}>
            {item.status === 'downloading' && <FileArrowDown size={20} />}
            {item.status === 'completed' && <CheckCircle size={20} />}
            {item.status === 'failed' && <WarningCircle size={20} />}
            {item.status === 'canceled' && <XCircle size={20} />}
          </div>
        )}
      </div>

      <div className="download-details">
        <div className="download-name-row">
          <h3 className="download-name" title={item.fileName}>{item.fileName}</h3>
          <span className={`download-percentage status-${item.status}`}>
            {item.status === 'downloading' ? `${Math.round(item.progress)}%` :
             item.status === 'completed' ? '100%' :
             item.status === 'canceled' ? 'Cancelado' : 'Erro'}
          </span>
        </div>

        <div className={`progress-bar-container ${item.status}`}>
          <div
            className="progress-bar-fill"
            style={{ width: `${item.status === 'completed' || item.status === 'failed' || item.status === 'canceled' ? 100 : item.progress}%` }}
          />
        </div>

        <div className="download-stats">
          {downloadMetaItems(item).map(meta => (
            <span key={meta} className="stat-item">{meta}</span>
          ))}
          {item.status === 'completed' && <span className="stat-item success-text">Finalizado</span>}
          {item.status === 'canceled' && <span className="stat-item muted-text">Cancelado</span>}
          {item.error && <span className="stat-item error-text">{item.error}</span>}
        </div>
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
        <button className="folder-action-btn" onClick={() => handleOpenFolder()} title="Abrir pasta de downloads">
          <FolderOpen size={16} /> Pasta
        </button>
      </header>

      <div className="downloads-content">
        <section className="quick-import-card">
          <div className="quick-import-header">
            <div className="quick-import-title">
              <Link size={16} /> Quick Import
            </div>

            <div className="supported-tags">
              <span className="supported-label">SUPPORTED:</span>
              <span className="supported-icon" title="YouTube" aria-label="YouTube"><YoutubeLogo size={18} weight="fill" /></span>
              <span className="supported-icon" title="Reddit" aria-label="Reddit"><RedditLogo size={18} weight="fill" /></span>
              <span className="supported-icon" title="Twitter/X" aria-label="Twitter/X"><TwitterLogo size={18} weight="fill" /></span>
              <span className="supported-icon" title="Instagram" aria-label="Instagram"><InstagramLogo size={18} weight="fill" /></span>
            </div>
          </div>

          <div className="downloader-input-group">
            <input
              type="text"
              placeholder="Paste YouTube, Reddit, or Twitter URL here..."
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && handleAnalyze()}
            />
            <button
              className="analyze-btn"
              onClick={handleAnalyze}
              disabled={analyzing || !url.trim()}
            >
              {analyzing ? <Spinner className="spin" size={18} /> : (
                <>Download <CloudArrowDown size={18} /></>
              )}
            </button>
          </div>

        {error && <div className="error-message">{error}</div>}

        {media && (
          <div className="media-preview-card">
            {media.thumbnailUrl && (
              <img src={media.thumbnailUrl} alt="Thumbnail" className="media-thumbnail" />
            )}
            <div className="media-info">
              <h3>{media.title}</h3>
              <p className="media-author">{media.author} • {media.duration}</p>
              
              <div className="format-selection">
                <select 
                  value={selectedFormat} 
                  onChange={(e) => setSelectedFormat(e.target.value)}
                >
                  <optgroup label="Vídeo">
                    {media.formats.video.map(f => (
                      <option key={f.id} value={f.id} disabled={!canDownloadFormat(media, f)}>
                        {f.label} {f.size !== '—' ? `(${f.size})` : ''}
                      </option>
                    ))}
                  </optgroup>
                  <optgroup label="Áudio">
                    {media.formats.audio.map(f => (
                      <option key={f.id} value={f.id} disabled={!canDownloadFormat(media, f)}>
                        {f.label} {f.size !== '—' ? `(${f.size})` : ''}
                      </option>
                    ))}
                  </optgroup>
                </select>
                <button className="download-btn" onClick={handleDownload} disabled={!selectedDownloadable}>
                  <CloudArrowDown size={18} /> Confirmar
                </button>
              </div>
            </div>
          </div>
        )}
        </section>

        <div className="downloads-section-header">
          <h2 className="downloads-section-title">
            Active Downloads
            {runningCount > 0 && (
              <span className="running-badge">{runningCount} RUNNING</span>
            )}
          </h2>
        </div>

        {downloads.length === 0 ? (
          <div className="empty-state">
            <HardDrive size={52} />
            <p>Nenhum download em andamento.</p>
          </div>
        ) : (
          <div className="downloads-list">
            {groupedDownloads.map(entry => {
              if (entry.type === 'single') return renderDownloadItem(entry.item);

              const summary = groupSummary(entry.items);
              const first = entry.items[0];
              const expanded = expandedGroups.has(entry.id);
              const meta = downloadMetaItems(first);

              return (
                <div key={entry.id} className={`download-group ${summary.status}`}>
                  <button type="button" className="download-group-header" onClick={() => toggleGroup(entry.id)}>
                    <div className="download-icon-wrapper">
                      {summary.thumbnail ? (
                        <div className="download-thumbnail-container">
                          <img src={summary.thumbnail} alt="Thumbnail" className="download-thumbnail-img" />
                          <div className={`download-status-overlay ${summary.status}`}>
                            {summary.status === 'downloading' && <FileArrowDown size={14} />}
                            {summary.status === 'completed' && <CheckCircle size={14} />}
                            {summary.status === 'failed' && <WarningCircle size={14} />}
                            {summary.status === 'canceled' && <XCircle size={14} />}
                          </div>
                        </div>
                      ) : (
                        <div className={`download-icon ${summary.status}`}>
                          <StackSimple size={20} />
                        </div>
                      )}
                    </div>

                    <div className="download-details">
                      <div className="download-name-row">
                        <h3 className="download-name" title={first.batchTitle || first.fileName}>
                          {first.batchTitle || 'Mass Download'}
                        </h3>
                        <span className={`download-percentage status-${summary.status}`}>
                          {summary.status === 'failed' ? 'Erro' : summary.status === 'canceled' ? 'Cancelado' : `${summary.progress}%`}
                        </span>
                      </div>

                      <div className={`progress-bar-container ${summary.status}`}>
                        <div className="progress-bar-fill" style={{ width: `${summary.progress}%` }} />
                      </div>

                      <div className="download-stats">
                        <span className="stat-item">{summary.total} arquivos</span>
                        <span className="stat-item">{summary.completed} finalizados</span>
                        {summary.skipped > 0 && <span className="stat-item">{summary.skipped} ignorados</span>}
                        {summary.running > 0 && <span className="stat-item">{summary.running} em andamento</span>}
                        {summary.failed > 0 && <span className="stat-item error-text">{summary.failed} falharam</span>}
                        {summary.canceled > 0 && <span className="stat-item muted-text">{summary.canceled} cancelados</span>}
                        {meta.map(item => <span key={item} className="stat-item">{item}</span>)}
                      </div>
                    </div>

                    <CaretDown className={`download-group-caret ${expanded ? 'open' : ''}`} size={18} />
                  </button>

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
