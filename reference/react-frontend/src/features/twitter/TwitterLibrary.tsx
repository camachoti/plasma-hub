import { useEffect, useState } from 'react';
import { IconAlertCircle, IconBrandX, IconDownload, IconLink, IconMessageCircle, IconRefresh, IconSearch, IconX } from "../../design-system/icons";
import { LoadingIndicator } from "../../design-system";
import { invokeCommand as invoke, listenEvent as listen } from '../../shared/platform/tauri';
import { runtimeCapabilities } from '../../shared/platform/runtime';
import { analyzeUrl, downloadMedia } from '../downloader/downloader';
import { downloadService, type DownloadItem } from '../downloader/DownloadService';
import type { MediaInfo } from '../downloader/types';
import { createTwitterProfileChat } from '../telegram/TwitterFakeChatStore';
import { getStoredTwitterCookies, loadStoredTwitterCookies, onTwitterSettingsChanged } from './TwitterSettingsStore';
import { Select, TextField } from '../../design-system';
import { EmptyState } from '../../design-system/EmptyState';
import { DownloadArtwork } from '../downloader/DownloadArtwork';
import '../../styles/TwitterLibrary.css';

interface TwitterProfileInfo {
  platform: 'twitter';
  username: string;
  displayName?: string;
  avatarUrl?: string;
  thumbnailUrl?: string;
  mediaCount: number;
  mediaUrls: string[];
  mediaItems?: Array<{
    url: string;
    thumbnailUrl?: string | null;
    isVideo: boolean;
  }>;
  originalUrl: string;
}

function getTwitterProfileUsername(input: string): string | null {
  const cleanInput = input.trim();
  const handleMatch = cleanInput.match(/^@([A-Za-z0-9_]{1,15})$/);
  if (handleMatch) return handleMatch[1];
  if (/^[A-Za-z0-9_]{1,15}$/.test(cleanInput)) return cleanInput;

  try {
    const parsed = new URL(cleanInput);
    const host = parsed.hostname.toLowerCase().replace(/^www\./, '');
    if (host !== 'x.com' && host !== 'twitter.com') return null;
    const parts = parsed.pathname.split('/').filter(Boolean);
    if (parts.length !== 1) return null;
    return /^[A-Za-z0-9_]{1,15}$/.test(parts[0]) ? parts[0] : null;
  } catch {
    const match = cleanInput.match(/(?:x\.com|twitter\.com)\/([A-Za-z0-9_]{1,15})\/?$/);
    return match?.[1] ?? null;
  }
}

function toTwitterProfileUrl(username: string): string {
  return `https://x.com/${username}`;
}

function getAnalysisErrorMessage(error: unknown, fallback: string): string {
  if (typeof error === 'string' && error.trim()) return error;
  if (error instanceof Error && error.message) return error.message;
  if (error && typeof error === 'object' && 'message' in error && typeof error.message === 'string') {
    return error.message;
  }
  return fallback;
}

export function TwitterLibrary() {
  const [url, setUrl] = useState('');
  const [cookies, setCookies] = useState(getStoredTwitterCookies);
  const [media, setMedia] = useState<MediaInfo | null>(null);
  const [profile, setProfile] = useState<TwitterProfileInfo | null>(null);
  const [selectedFormat, setSelectedFormat] = useState('');
  const [downloads, setDownloads] = useState<DownloadItem[]>([]);
  const [analyzing, setAnalyzing] = useState(false);
  const [downloading, setDownloading] = useState(false);
  const [creatingChat, setCreatingChat] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setDownloads(Array.from(downloadService.activeDownloads.values()).filter(item => item.platform === 'twitter').reverse());
    const unsubscribe = downloadService.onDownloadsChange((list: DownloadItem[]) => {
      setDownloads(list.filter(item => item.platform === 'twitter').reverse());
    });
    return () => unsubscribe();
  }, []);

  useEffect(() => onTwitterSettingsChanged(() => setCookies(getStoredTwitterCookies())), []);
  useEffect(() => {
    loadStoredTwitterCookies().then(setCookies).catch(() => {});
  }, []);

  async function handleAnalyze() {
    const cleanUrl = url.trim();
    if (!cleanUrl) return;

    setAnalyzing(true);
    setError(null);
    setMedia(null);
    setProfile(null);
    setSelectedFormat('');
    const profileUsername = getTwitterProfileUsername(cleanUrl);

    try {
      const currentCookies = await loadStoredTwitterCookies();
      if (profileUsername) {
        const info = await invoke<TwitterProfileInfo>('analyze_twitter_profile_native', {
          url: toTwitterProfileUrl(profileUsername),
          cookies: currentCookies.trim() || null,
        });
        setProfile(info);
        return;
      }

      const info = await analyzeUrl(cleanUrl, { twitterCookies: cookies });
      if (info.platform !== 'twitter') {
        throw new Error('Cole uma URL do Twitter/X.');
      }

      setMedia(info);
      const firstPlayable = info.formats.video.find(format => format.url);
      if (firstPlayable) setSelectedFormat(firstPlayable.id);
    } catch (err: any) {
      setError(getAnalysisErrorMessage(err, profileUsername
        ? 'Não foi possível analisar esse perfil do Twitter/X.'
        : 'Não foi possível analisar esse tweet.'));
    } finally {
      setAnalyzing(false);
    }
  }

  async function handleDownload() {
    if (!media || !selectedFormat) return;

    setDownloading(true);
    setError(null);
    try {
      await downloadMedia(media, 'video', selectedFormat, { twitterCookies: cookies });
    } catch (err: any) {
      setError(err?.message || 'Falha ao baixar mídia.');
    } finally {
      setDownloading(false);
    }
  }

  async function handleProfileDownload() {
    if (!profile || profile.mediaUrls.length === 0) return;
    if (!runtimeCapabilities.supportsNativeTwitter) {
      setError('Download nativo do Twitter/X disponível apenas no app Tauri.');
      return;
    }

    const downloadId = `twitter_profile_${profile.username}_${Date.now()}`;
    const fileName = `@${profile.username}`;
    setDownloading(true);
    setError(null);
    downloadService.addDownload({
      id: downloadId,
      fileName,
      progress: 0,
      status: 'downloading',
      platform: 'twitter',
      thumbnailUrl: profile.thumbnailUrl || profile.avatarUrl,
    }, {
      cancel: async () => {
        await invoke<boolean>('cancel_native_download', { id: downloadId });
        downloadService.updateDownload(downloadId, {
          status: 'canceled',
          progress: 0,
          error: 'Cancelado pelo usuário.',
        });
      },
      retry: () => handleProfileDownload(),
    });

    const unlisteners: Array<() => void> = [];
    try {
      const unlistenProgress = await listen<{id: string, progress: number}>('twitter-download-progress', (e) => {
        if (e.payload.id === downloadId) {
          downloadService.updateDownload(downloadId, { progress: e.payload.progress });
        }
      });
      unlisteners.push(unlistenProgress);
      const unlistenDone = await listen<{id: string}>('twitter-download-done', (e) => {
        if (e.payload.id === downloadId) {
          downloadService.updateDownload(downloadId, { status: 'completed', progress: 100 });
        }
      });
      unlisteners.push(unlistenDone);
      const unlistenError = await listen<{id: string, error: string}>('twitter-download-error', (e) => {
        if (e.payload.id === downloadId) {
          downloadService.updateDownload(downloadId, { status: 'failed', error: e.payload.error });
        }
      });
      unlisteners.push(unlistenError);

      await invoke('download_twitter_profile_native', {
        id: downloadId,
        username: profile.username,
        mediaUrls: profile.mediaUrls,
        cookies: cookies.trim() || null,
      });

    } catch (err: any) {
      if (downloadService.getDownload(downloadId)?.status === 'canceled') return;
      downloadService.updateDownload(downloadId, { status: 'failed', error: err?.message || String(err) });
      setError(err?.message || 'Falha ao baixar mídias do perfil.');
    } finally {
      unlisteners.forEach(unlisten => unlisten());
      downloadService.clearActions(downloadId, ['cancel']);
      setDownloading(false);
    }
  }

  async function handleCreateProfileChat() {
    if (!profile || profile.mediaUrls.length === 0) return;

    setCreatingChat(true);
    setError(null);
    try {
      const res = createTwitterProfileChat(profile);
      if (!res.success) {
        throw new Error(res.error || 'Falha ao criar chat do perfil.');
      }
    } catch (err: any) {
      setError(err?.message || 'Falha ao criar chat do perfil.');
    } finally {
      setCreatingChat(false);
    }
  }

  const selected = media?.formats.video.find(format => format.id === selectedFormat);
  const canDownload = Boolean(media && selected?.url);
  const canDownloadProfile = Boolean(profile && profile.mediaUrls.length > 0);
  const runningCount = downloads.filter(item => item.status === 'downloading').length;

  return (
    <div className="twitter-library twitter-library-single">
      <main className="twitter-main">
        <header className="twitter-topbar">
          <div className="twitter-title-block">
            <span className="twitter-topbar-icon">
              <IconBrandX size={18} stroke={2} />
            </span>
            <h1>Twitter / X</h1>
            <span>{downloads.length} downloads</span>
          </div>
        </header>

        <div className="twitter-content">
          <section className="twitter-panel twitter-tweet-tool">
            <div className="twitter-tool-header">
              <label htmlFor="twitter-url">URL do tweet/status ou @perfil</label>
              {cookies.trim() && (
                <span className="twitter-cookies-saved">Cookies configurados</span>
              )}
            </div>
            <div className="twitter-url-row">
              <div className="twitter-input-row">
                <IconLink size={18} stroke={2} />
                <TextField
                  appearance="inline"
                  id="twitter-url"
                  value={url}
                  onChange={event => setUrl(event.target.value)}
                  onKeyDown={event => event.key === 'Enter' && handleAnalyze()}
                  placeholder="https://x.com/usuario/status/123456789 ou @usuario"
                />
              </div>
              <button className="twitter-primary-btn" onClick={handleAnalyze} disabled={analyzing || !url.trim()} aria-busy={analyzing}>
                {analyzing ? <LoadingIndicator size="sm" /> : <IconSearch size={18} stroke={2} />}
                <span>{analyzing ? 'Analisando' : 'Analisar'}</span>
              </button>
            </div>
            {error && (
              <div className="twitter-error">
                <IconAlertCircle size={17} stroke={2} />
                <span>{error}</span>
              </div>
            )}
          </section>

          {media && (
            <section className="twitter-preview">
              {media.thumbnailUrl && (
                <img src={media.thumbnailUrl} alt="" className="twitter-preview-thumb" />
              )}
              <div className="twitter-preview-info">
                <p>{media.author} {media.duration !== '—' ? `· ${media.duration}` : ''}</p>
                <h3>{media.title}</h3>
                <div className="twitter-format-row">
                  <Select value={selectedFormat} onChange={setSelectedFormat} ariaLabel="Formato do vídeo" options={media.formats.video.map(format => ({ value: format.id, label: `${format.label} ${format.size !== '—' ? `(${format.size})` : ''}`, disabled: !format.url }))} />
                  <button className="twitter-primary-btn" onClick={handleDownload} disabled={!canDownload || downloading}>
                    {downloading ? <LoadingIndicator size="sm" /> : <IconDownload size={18} stroke={2} />}
                    <span>{downloading ? 'Baixando' : 'Baixar'}</span>
                  </button>
                </div>
              </div>
            </section>
          )}

          {profile && (
            <section className="twitter-preview">
              {(profile.thumbnailUrl || profile.avatarUrl) && (
                <img
                  src={profile.thumbnailUrl || profile.avatarUrl}
                  alt=""
                  className={`twitter-preview-thumb ${profile.thumbnailUrl ? '' : 'twitter-profile-avatar'}`}
                />
              )}
              <div className="twitter-preview-info">
                <p>@{profile.username}</p>
                <h3>{profile.displayName || `Perfil @${profile.username}`}</h3>
                <div className="twitter-profile-stats">
                  <strong>{profile.mediaCount}</strong>
                  <span>{profile.mediaCount === 1 ? 'mídia encontrada' : 'mídias encontradas'}</span>
                </div>
                <div className="twitter-format-row">
                  <button className="twitter-primary-btn" onClick={handleProfileDownload} disabled={!canDownloadProfile || downloading}>
                    {downloading ? <LoadingIndicator size="sm" /> : <IconDownload size={18} stroke={2} />}
                    <span>{downloading ? 'Baixando' : 'Baixar mídias'}</span>
                  </button>
                  <button className="twitter-secondary-btn" onClick={handleCreateProfileChat} disabled={!canDownloadProfile || creatingChat}>
                    {creatingChat ? <LoadingIndicator size="sm" /> : <IconMessageCircle size={18} stroke={2} />}
                    <span>{creatingChat ? 'Criando' : 'Criar chat'}</span>
                  </button>
                </div>
              </div>
            </section>
          )}

          <section className="twitter-activity">
            <div className="twitter-activity-header">
              <h3>
                Downloads
                {runningCount > 0 && <span className="twitter-running-badge">{runningCount} em andamento</span>}
              </h3>
            </div>
            <div className="twitter-feed">
              {downloads.length === 0 ? (
                <EmptyState
                  className="twitter-feed-empty"
                  icon={<IconBrandX size={30} stroke={1.5} />}
                  title="Nenhum download nesta sessão"
                  description="Analise um link ou perfil acima para baixar suas mídias do Twitter/X."
                />
              ) : (
                downloads.map(item => (
                  <div className={`twitter-feed-item ${item.status}`} key={item.id}>
                    <DownloadArtwork item={item} />
                    <div className="twitter-feed-details">
                      <div className="twitter-feed-name-row">
                        <h4 title={item.fileName}>{item.fileName}</h4>
                        <span className={`twitter-feed-status ${item.status}`}>
                          {item.status === 'downloading' ? `${Math.round(item.progress)}%` :
                            item.status === 'completed' ? 'Concluído' :
                              item.status === 'canceled' ? 'Cancelado' : 'Erro'}
                        </span>
                      </div>
                      {item.status === 'downloading' && (
                        <div className="twitter-feed-progress" role="progressbar" aria-label={`Progresso de ${item.fileName}`} aria-valuenow={Math.round(item.progress)} aria-valuemin={0} aria-valuemax={100}>
                          <div style={{ width: `${Math.max(0, Math.min(100, item.progress))}%` }} />
                        </div>
                      )}
                      <p className="twitter-feed-meta">Twitter / X{item.sourceLabel ? ` · ${item.sourceLabel}` : ''}{item.error ? ` · ${item.error}` : ''}</p>
                    </div>
                    {item.status === 'downloading' && item.canCancel && (
                      <button type="button" className="twitter-feed-action" aria-label={`Cancelar ${item.fileName}`} title="Cancelar download" onClick={() => void downloadService.cancelDownload(item.id)}>
                        <IconX size={17} stroke={2} />
                      </button>
                    )}
                    {(item.status === 'failed' || item.status === 'canceled') && item.canRetry && (
                      <button type="button" className="twitter-feed-action" aria-label={`Tentar novamente ${item.fileName}`} title="Tentar novamente" onClick={() => void downloadService.retryDownload(item.id)}>
                        <IconRefresh size={17} stroke={2} />
                      </button>
                    )}
                  </div>
                ))
              )}
            </div>
          </section>
        </div>
      </main>
    </div>
  );
}
