import React, { useEffect, useState } from 'react';
import { CloudArrowDown, CheckCircle, WarningCircle, FileArrowDown, HardDrive, Link, Spinner, FolderOpen, YoutubeLogo, RedditLogo, TwitterLogo, InstagramLogo } from '@phosphor-icons/react';
import '../../styles/Downloads.css';
import { downloadService, DownloadItem } from '../downloader/DownloadService';
import { analyzeUrl, downloadMedia } from '../downloader/downloader';
import type { MediaInfo } from '../downloader/types';
import { downloadDir, join } from '@tauri-apps/api/path';
import { openPath, revealItemInDir } from '@tauri-apps/plugin-opener';

export const Downloads: React.FC = () => {
  const [downloads, setDownloads] = useState<DownloadItem[]>([]);
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
    if (media) {
      const currentValid = media.formats.video.some(f => f.id === selectedFormat) || media.formats.audio.some(f => f.id === selectedFormat);
      if (!currentValid && media.formats.video.length > 0) {
        setSelectedFormat(media.formats.video[0].id);
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
      if (info.formats.video.length > 0) {
        setSelectedFormat(info.formats.video[0].id);
      } else if (info.formats.audio.length > 0) {
        setSelectedFormat(info.formats.audio[0].id);
      }
    } catch (err: any) {
      setError(err.message || 'Erro ao analisar URL');
    } finally {
      setAnalyzing(false);
    }
  };

  const handleDownload = () => {
    if (!media || !selectedFormat) return;
    const mode = media.formats.video.some(f => f.id === selectedFormat) ? 'video' : 'audio';
    downloadMedia(media, mode, selectedFormat);
    setUrl('');
    setMedia(null);
  };

  const handleOpenFolder = async (item?: DownloadItem) => {
    try {
      if (item?.filePath) {
        const parentDir = item.filePath.split(/[\\/]/).slice(0, -1).join('/') || item.filePath;
        await revealItemInDir(item.filePath).catch(() => openPath(parentDir));
        return;
      }

      const dir = await downloadDir();
      if (item) {
        const fullPath = await join(dir, item.fileName);
        await revealItemInDir(fullPath).catch(() => openPath(dir));
      } else {
        await openPath(dir);
      }
    } catch (e) {
      console.error('Failed to open folder:', e);
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

  const downloadMetaItems = (item: DownloadItem) => [
    platformLabel(item.platform),
    item.chatTitle ? `${item.chatKind === 'grupo' ? 'Grupo' : item.chatKind === 'canal' ? 'Canal' : item.chatKind === 'twitter' ? 'Twitter' : 'Chat'}: ${item.chatTitle}` : null,
    item.topicTitle ? `Tópico: ${item.topicTitle}` : null,
    item.senderName ? `Usuário: ${item.senderName}` : null,
  ].filter(Boolean);

  const runningCount = downloads.filter(d => d.status === 'downloading').length;

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
                      <option key={f.id} value={f.id} disabled={f.id === 'na' || f.id === 'web-limit'}>
                        {f.label} {f.size !== '—' ? `(${f.size})` : ''}
                      </option>
                    ))}
                  </optgroup>
                  <optgroup label="Áudio">
                    {media.formats.audio.map(f => (
                      <option key={f.id} value={f.id} disabled={f.id === 'na' || f.id === 'web-limit'}>
                        {f.label} {f.size !== '—' ? `(${f.size})` : ''}
                      </option>
                    ))}
                  </optgroup>
                </select>
                <button className="download-btn" onClick={handleDownload}>
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
            {downloads.map(item => (
              <div
                key={item.id}
                className={`download-item ${item.status === 'completed' ? 'clickable' : ''}`}
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
                      </div>
                    </div>
                  ) : (
                    <div className={`download-icon ${item.status}`}>
                      {item.status === 'downloading' && <FileArrowDown size={20} />}
                      {item.status === 'completed' && <CheckCircle size={20} />}
                      {item.status === 'failed' && <WarningCircle size={20} />}
                    </div>
                  )}
                </div>

                <div className="download-details">
                  <div className="download-name-row">
                    <h3 className="download-name" title={item.fileName}>{item.fileName}</h3>
                    <span className={`download-percentage status-${item.status}`}>
                      {item.status === 'downloading' ? `${Math.round(item.progress)}%` :
                       item.status === 'completed' ? '100%' : 'Error'}
                    </span>
                  </div>

                  <div className={`progress-bar-container ${item.status}`}>
                    <div
                      className="progress-bar-fill"
                      style={{ width: `${item.status === 'completed' ? 100 : item.status === 'failed' ? 100 : item.progress}%` }}
                    />
                  </div>

                  <div className="download-stats">
                    {downloadMetaItems(item).map(meta => (
                      <span key={meta} className="stat-item">{meta}</span>
                    ))}
                    {item.status === 'completed' && <span className="stat-item success-text">Finalizado</span>}
                    {item.error && <span className="stat-item error-text">{item.error}</span>}
                  </div>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
};
